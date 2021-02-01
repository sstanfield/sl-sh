use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::num::{ParseFloatError, ParseIntError};

use unicode_segmentation::UnicodeSegmentation;

use crate::builtins_hashmap::cow_to_ref;
use crate::environment::*;
use crate::eval::eval;
use crate::types::*;

#[derive(Clone, Debug)]
pub struct ReadError {
    pub reason: String,
}

impl Error for ReadError {}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.reason)
    }
}

enum ListType {
    Vector,
    List,
}

struct List {
    list_type: ListType,
    vec: Vec<Expression>,
}

fn is_whitespace(ch: &str) -> bool {
    matches!(ch, " " | "\t" | "\n")
}

fn char_to_hex_num(ch: &str) -> Result<u8, ReadError> {
    if ("0"..="9").contains(&ch) {
        Ok(ch.chars().next().unwrap() as u8 - b'0')
    } else {
        match ch {
            "a" => Ok(10),
            "A" => Ok(10),
            "b" => Ok(11),
            "B" => Ok(11),
            "c" => Ok(12),
            "C" => Ok(12),
            "d" => Ok(13),
            "D" => Ok(13),
            "e" => Ok(14),
            "E" => Ok(14),
            "f" => Ok(15),
            "F" => Ok(15),
            _ => Err(ReadError {
                reason: format!("Invalid hex digit {}, expected 0-9 or A-F.", ch),
            }),
        }
    }
}

fn escape_to_char(chars: &mut CharIter, reader_state: &mut ReaderState) -> Result<char, ReadError> {
    if let (Some(ch1), Some(ch2)) = (chars.next(), chars.next()) {
        reader_state.column += 1;
        let ch_n: u8 = (char_to_hex_num(&*ch1)? * 16) + (char_to_hex_num(&*ch2)?);
        if ch_n > 0x7f {
            Err(ReadError {
                reason: "Invalid hex ascii code, must be less then \\x7f.".to_string(),
            })
        } else {
            Ok(ch_n as char)
        }
    } else {
        Err(ReadError {
            reason: "Invalid hex ascii code, expected two digits.".to_string(),
        })
    }
}

fn close_list(stack: &mut Vec<List>, exp_meta: Option<ExpMeta>) -> Result<(), ReadError> {
    match stack.pop() {
        Some(v) => match stack.pop() {
            Some(mut v2) => {
                match v.list_type {
                    ListType::Vector => {
                        v2.vec.push(Expression::with_list(v.vec));
                    }
                    ListType::List => {
                        if v.vec.len() == 3 && v.vec[1].to_string() == "." {
                            v2.vec.push(Expression::alloc(ExpObj {
                                data: ExpEnum::Pair(v.vec[0].clone(), v.vec[2].clone()),
                                meta: exp_meta,
                                meta_tags: None,
                                analyzed: RefCell::new(false),
                            }));
                        } else {
                            v2.vec.push(Expression::cons_from_vec(&v.vec, exp_meta));
                        }
                    }
                }
                stack.push(v2);
            }
            None => {
                stack.push(v);
            }
        },
        None => {
            return Err(ReadError {
                reason: "Unexpected `)`".to_string(),
            });
        }
    }
    Ok(())
}

fn get_meta(name: Option<&'static str>, line: usize, col: usize) -> Option<ExpMeta> {
    if let Some(file) = name {
        Some(ExpMeta { file, line, col })
    } else {
        None
    }
}

fn consume_line_comment(chars: &mut CharIter, reader_state: &mut ReaderState) {
    for ch in chars {
        if ch == "\n" {
            reader_state.line += 1;
            reader_state.column = 0;
            return;
        }
    }
}

fn consume_block_comment(chars: &mut CharIter, reader_state: &mut ReaderState) {
    let mut depth = 1;
    let mut last_ch = Cow::Borrowed(" ");
    for ch in chars {
        if ch == "\n" {
            reader_state.line += 1;
            reader_state.column = 0;
        } else {
            reader_state.column += 1;
        }
        if last_ch == "|" && ch == "#" {
            depth -= 1;
        }
        if last_ch == "#" && ch == "|" {
            depth += 1;
        }
        last_ch = ch;
        if depth == 0 {
            break;
        }
    }
}

fn end_symbol(ch: &str, in_back_quote: bool, reader_state: &mut ReaderState) -> bool {
    if is_whitespace(ch) || (reader_state.end_ch.is_some() && ch == reader_state.end_ch.unwrap()) {
        true
    } else {
        match ch {
            "(" => true,
            ")" => true,
            "#" => true,
            "\"" => true,
            "," if in_back_quote => true,
            "'" => true,
            "`" => true,
            _ => false,
        }
    }
}

fn is_digit(ch: &str) -> bool {
    matches!(
        ch,
        "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"
    )
}

fn do_char(environment: &mut Environment, symbol: &str) -> Result<Expression, ReadError> {
    match &symbol.to_lowercase()[..] {
        "space" => return Ok(Expression::alloc_data(ExpEnum::Char(" ".into()))),
        "tab" => return Ok(Expression::alloc_data(ExpEnum::Char("\t".into()))),
        // newline should be the platform line end.
        "newline" => return Ok(Expression::alloc_data(ExpEnum::Char("\n".into()))),
        "linefeed" => return Ok(Expression::alloc_data(ExpEnum::Char("\n".into()))),
        "return" => return Ok(Expression::alloc_data(ExpEnum::Char("\r".into()))),
        "backspace" => return Ok(Expression::alloc_data(ExpEnum::Char("\u{0008}".into()))),
        _ => {}
    }
    // Do this so the chars iterator has a static lifetime.  Should be ok since
    // iterator dies at the end of the function and symbol does not.
    // Note: interning the chars below keeps from using the temp buffer.
    let ntext = unsafe { &*(symbol as *const str) };
    let mut chars: CharIter = Box::new(
        UnicodeSegmentation::graphemes(ntext, true)
            .map(|s| Cow::Borrowed(s))
            .peekable(),
    );
    if let Some(ch) = chars.next() {
        if chars.peek().is_some() {
            match &*ch {
                "u" => {
                    let reader_state = &mut environment.reader_state.as_mut().unwrap();
                    let char_str = format!("{}", read_utf_scalar(&mut chars, reader_state,)?);
                    return Ok(Expression::alloc_data(ExpEnum::Char(char_str.into())));
                }
                "x" => {
                    let reader_state = &mut environment.reader_state.as_mut().unwrap();
                    let char_str = format!("{}", escape_to_char(&mut chars, reader_state,)?);
                    return Ok(Expression::alloc_data(ExpEnum::Char(char_str.into())));
                }
                _ => {
                    let reader_state = &mut environment.reader_state.as_mut().unwrap();
                    let reason = format!(
                        "Not a valid char [{}]: line {}, col: {}",
                        symbol, reader_state.line, reader_state.column
                    );
                    return Err(ReadError { reason });
                }
            }
        }
        Ok(Expression::alloc_data(ExpEnum::Char(
            environment.interner.intern(&*ch).into(),
        )))
    } else {
        let reader_state = &mut environment.reader_state.as_mut().unwrap();
        let reason = format!(
            "Not a valid char [{}]: line {}, col: {}",
            symbol, reader_state.line, reader_state.column
        );
        Err(ReadError { reason })
    }
}

fn read_utf_scalar(
    chars: &mut CharIter,
    reader_state: &mut ReaderState,
) -> Result<char, ReadError> {
    fn finish(char_u32: u32) -> Result<char, ReadError> {
        if let Some(val) = std::char::from_u32(char_u32) {
            Ok(val)
        } else {
            Err(ReadError {
                reason: format!(
                    "Invalid unicode scalar, {:x} not a valid utf scalar.",
                    char_u32
                ),
            })
        }
    }
    let mut first = true;
    let mut has_bracket = false;
    let mut char_u32 = 0;
    let mut nibbles = 0;
    let mut out_ch = chars.next();
    while let Some(ch) = out_ch {
        if ch == "\n" {
            reader_state.line += 1;
            reader_state.column = 0;
            if has_bracket {
                return Err(ReadError {
                    reason: "Invalid unicode scalar, unexpected newline.".to_string(),
                });
            } else {
                return finish(char_u32);
            }
        } else {
            reader_state.column += 1;
        }
        if first && ch == "{" {
            has_bracket = true;
            out_ch = chars.next();
            first = false;
            continue;
        }
        first = false;
        if has_bracket && ch == "}" {
            return finish(char_u32);
        }
        if nibbles >= 8 {
            return Err(ReadError {
                reason: "Invalid unicode scalar, too many bytes (4 max).".to_string(),
            });
        }
        nibbles += 1;
        let nib = char_to_hex_num(&ch)?;
        char_u32 = (char_u32 << 4) | nib as u32;
        if let Some(pch) = chars.peek() {
            if !has_bracket && is_whitespace(&*pch) {
                return finish(char_u32);
            }
        }
        out_ch = chars.next();
    }
    if has_bracket {
        Err(ReadError {
            reason: "Invalid unicode scalar, failed to parse.".to_string(),
        })
    } else {
        finish(char_u32)
    }
}

fn read_string(
    chars: &mut CharIter,
    symbol: &mut String,
    reader_state: &mut ReaderState,
) -> Result<Expression, ReadError> {
    symbol.clear();
    let mut last_ch = Cow::Borrowed(" ");
    let mut skip_last_ch = false;

    let mut out_ch = chars.next();
    while let Some(ch) = out_ch {
        if ch == "\n" {
            reader_state.line += 1;
            reader_state.column = 0;
        } else {
            reader_state.column += 1;
        }
        if last_ch == "\\" {
            match &*ch {
                "n" => symbol.push('\n'),
                "r" => symbol.push('\r'),
                "t" => symbol.push('\t'),
                "\"" => symbol.push('"'),
                "x" => symbol.push(escape_to_char(chars, reader_state)?),
                "\\" => {
                    skip_last_ch = true;
                    symbol.push('\\');
                }
                "u" => symbol.push(read_utf_scalar(chars, reader_state)?),
                _ => {
                    symbol.push('\\');
                    symbol.push_str(&ch);
                }
            }
        } else {
            if ch == "\"" {
                break;
            }
            if ch != "\\" {
                symbol.push_str(&ch);
            }
        }

        last_ch = if skip_last_ch {
            skip_last_ch = false;
            Cow::Borrowed(" ")
        } else {
            ch
        };
        out_ch = chars.next();
    }
    Ok(Expression::alloc_data(ExpEnum::String(
        symbol.clone().into(),
        None,
    )))
}

fn do_atom(environment: &mut Environment, symbol: &str, is_number: bool) -> Expression {
    if is_number {
        let mut num_str = symbol.to_string();
        num_str.retain(|ch| ch != '_');
        let potential_int: Result<i64, ParseIntError> = num_str.parse();
        match potential_int {
            Ok(v) => Expression::alloc_data(ExpEnum::Int(v)),
            Err(_) => {
                let potential_float: Result<f64, ParseFloatError> = num_str.parse();
                match potential_float {
                    Ok(v) => Expression::alloc_data(ExpEnum::Float(v)),
                    Err(_) => Expression::alloc_data(ExpEnum::Symbol(
                        environment.interner.intern(symbol),
                        SymLoc::None,
                    )),
                }
            }
        }
    } else {
        if symbol.is_empty() {
            return Expression::alloc_data(ExpEnum::Nil);
        }
        if symbol == "t" {
            Expression::alloc_data(ExpEnum::True)
        } else if symbol == "nil" {
            Expression::alloc_data(ExpEnum::Nil)
        } else {
            Expression::alloc_data(ExpEnum::Symbol(
                environment.interner.intern(symbol),
                SymLoc::None,
            ))
        }
    }
}

fn push_stack(
    stack: &mut Vec<List>,
    expression: Expression,
    line: usize,
    column: usize,
) -> Result<(), ReadError> {
    match stack.pop() {
        Some(mut v) => {
            v.vec.push(expression);
            stack.push(v);
            Ok(())
        }
        None => {
            let reason = format!(
                "Found symbol without containing list: line {}, col: {}",
                line, column
            );
            Err(ReadError { reason })
        }
    }
}

fn read_symbol(
    buffer: &mut String,
    chars: &mut CharIter,
    reader_state: &mut ReaderState,
    for_ch: bool,
    in_back_quote: bool,
    skip_underscore: bool,
) -> bool {
    let mut has_peek;
    let mut push_next = false;
    let mut is_number = buffer.is_empty()
        || (buffer.len() == 1
            && (is_digit(&buffer[..])
                || (&buffer[..] == "+")
                || (&buffer[..] == "-")
                || (&buffer[..] == ".")));
    let mut decimals = if buffer.len() == 1 && &buffer[..] == "." {
        1
    } else {
        0
    };
    if let Some(ch) = chars.peek() {
        if end_symbol(&ch, in_back_quote, reader_state) && !for_ch {
            return buffer.len() == 1 && is_digit(&buffer[..]);
        }
    };
    let mut next_ch = chars.next();
    while next_ch.is_some() {
        let ch = next_ch.unwrap();
        let peek_ch = if let Some(pch) = chars.peek() {
            has_peek = true;
            &pch
        } else {
            has_peek = false;
            " "
        };
        if ch == "\n" {
            reader_state.line += 1;
            reader_state.column = 0;
        } else {
            reader_state.column += 1;
        }
        if ch == "\\" && has_peek && !for_ch {
            push_next = true;
        } else if !skip_underscore || ch != "_" {
            if !is_digit(&ch) && ch != "." && ch != "_" {
                is_number = false;
            }
            if ch == "." {
                decimals += 1;
            }
            buffer.push_str(&ch);
        }
        if push_next {
            let next_ch = chars.next().unwrap();
            if !is_digit(&next_ch) && next_ch != "." && next_ch != "_" {
                is_number = false;
            }
            if next_ch == "." {
                decimals += 1;
            }
            buffer.push_str(&next_ch);
            push_next = false;
        } else if end_symbol(peek_ch, in_back_quote, reader_state) {
            break;
        }
        next_ch = chars.next();
    }
    is_number && decimals <= 1
}

fn next2(chars: &mut CharIter) -> Option<(Cow<'static, str>, Cow<'static, str>)> {
    if let Some(ch) = chars.next() {
        let peek_ch = if let Some(pch) = chars.peek() {
            pch.clone()
        } else {
            Cow::Borrowed(" ")
        };
        Some((ch, peek_ch))
    } else {
        None
    }
}

fn call_reader_macro(
    environment: &mut Environment,
    name: &str,
    stream: Expression,
    ch: &str,
    end_ch: Option<&'static str>,
) -> Result<Expression, ReadError> {
    if let Some(exp) = lookup_expression(environment, name) {
        let exp = match &exp.get().data {
            ExpEnum::Lambda(_) => {
                let mut v = Vec::with_capacity(1);
                v.push(Expression::alloc_data(ExpEnum::Symbol(
                    environment.interner.intern(name),
                    SymLoc::None,
                )));
                v.push(stream);
                v.push(Expression::alloc_data(ExpEnum::Char(ch.to_string().into())));
                Expression::with_list(v)
            }
            _ => {
                let reason = format!(
                    "Error calling reader macro (not a lambda) {}, {} : line {}, col: {}",
                    name,
                    environment
                        .reader_state
                        .as_ref()
                        .unwrap()
                        .file_name
                        .unwrap_or(""),
                    environment.reader_state.as_ref().unwrap().line,
                    environment.reader_state.as_ref().unwrap().column
                );
                return Err(ReadError { reason });
            }
        };
        let old_end_ch = environment.reader_state.as_ref().unwrap().end_ch;
        environment.reader_state.as_mut().unwrap().end_ch = end_ch;
        let res = match eval(environment, exp) {
            Ok(exp) => Ok(exp),
            Err(err) => {
                let reason = format!(
                    "Error in reader {}: {} ({} : line {}, col: {})",
                    name,
                    err,
                    environment
                        .reader_state
                        .as_ref()
                        .unwrap()
                        .file_name
                        .unwrap_or(""),
                    environment.reader_state.as_ref().unwrap().line,
                    environment.reader_state.as_ref().unwrap().column
                );
                Err(ReadError { reason })
            }
        };
        environment.reader_state.as_mut().unwrap().end_ch = old_end_ch;
        res
    } else {
        let reason = format!(
            "Error calling reader macro (not found) {}, {} : line {}, col: {}",
            name,
            environment
                .reader_state
                .as_ref()
                .unwrap()
                .file_name
                .unwrap_or(""),
            environment.reader_state.as_ref().unwrap().line,
            environment.reader_state.as_ref().unwrap().column
        );
        Err(ReadError { reason })
    }
}

fn prep_reader_macro(
    environment: &mut Environment,
    chars: CharIter, // Pass ownership in and out for reader macro support.
    stack: &mut Vec<List>,
    name: &str,
    ch: &str,
    end_ch: Option<&'static str>,
) -> Result<CharIter, (ReadError, CharIter)> {
    fn recover_chars(stream_exp: &Expression) -> CharIter {
        let mut exp_d = stream_exp.get_mut();
        if let ExpEnum::String(_, chars_iter) = &mut exp_d.data {
            chars_iter.take().unwrap()
        } else {
            panic!("read: something happened to char iterator in reader macro!");
        }
    }
    let stream_exp = Expression::alloc_data(ExpEnum::String("".into(), Some(chars)));
    {
        let mut exp_d = stream_exp.get_mut();
        if let Some(tags) = &mut exp_d.meta_tags {
            tags.insert("--reader-text-stream--");
        } else {
            let mut tags: HashSet<&'static str> = HashSet::new();
            tags.insert("--reader-text-stream--");
            exp_d.meta_tags = Some(tags);
        }
    }
    let rm = match call_reader_macro(environment, name, stream_exp.clone(), ch, end_ch) {
        Ok(rm) => rm,
        Err(e) => {
            let chars = recover_chars(&stream_exp);
            return Err((e, chars));
        }
    };
    if let Err(e) = push_stack(
        stack,
        rm,
        environment.reader_state.as_ref().unwrap().line,
        environment.reader_state.as_ref().unwrap().column,
    ) {
        let chars = recover_chars(&stream_exp);
        return Err((e, chars));
    }
    let res = recover_chars(&stream_exp);
    // Clear the stream expression in case the reader macro saved it for some dumb reason.
    let mut exp_d = stream_exp.get_mut();
    exp_d.data.replace(ExpEnum::Nil);
    exp_d.meta_tags = None;
    Ok(res)
}

fn consume_trailing_whitespace(environment: &mut Environment, chars: &mut CharIter) {
    // Consume trailing whitespace.
    let mut ch = chars.peek();
    while ch.is_some() && is_whitespace(ch.unwrap()) {
        if let Some(ch) = ch {
            if *ch == "\n" {
                environment.reader_state.as_mut().unwrap().line += 1;
                environment.reader_state.as_mut().unwrap().column = 0;
            } else {
                environment.reader_state.as_mut().unwrap().column += 1;
            }
            chars.next();
        }
        ch = chars.peek();
    }
}

fn read_num_radix(
    environment: &mut Environment,
    mut chars: CharIter, // Pass ownership in and out for reader macro support.
    stack: &mut Vec<List>,
    buffer: &mut String,
    in_back_quote: bool,
    radix: u32,
) -> Result<CharIter, (ReadError, CharIter)> {
    buffer.clear();
    read_symbol(
        buffer,
        &mut chars,
        &mut environment.reader_state.as_mut().unwrap(),
        true,
        in_back_quote,
        true,
    );
    let num = match i64::from_str_radix(buffer, radix) {
        Ok(n) => Expression::alloc_data(ExpEnum::Int(n)),
        Err(e) => {
            return Err((
                ReadError {
                    reason: e.to_string(),
                },
                chars,
            ))
        }
    };
    if let Err(e) = push_stack(
        stack,
        num,
        environment.reader_state.as_ref().unwrap().line,
        environment.reader_state.as_ref().unwrap().column,
    ) {
        return Err((e, chars));
    }
    Ok(chars)
}

fn read_inner(
    environment: &mut Environment,
    mut chars: CharIter, // Pass ownership in and out for reader macro support.
    stack: &mut Vec<List>,
    buffer: &mut String,
    in_back_quote: bool,
) -> Result<(bool, CharIter), (ReadError, CharIter)> {
    if environment.reader_state.is_none() {
        panic!("tried to read with no state!");
    }
    let mut level = 0;
    let mut line_stack: Vec<(usize, usize)> = Vec::new();
    let mut next_chars = next2(&mut chars);
    let mut read_next = false;
    let read_table = lookup_expression(&environment, "*read-table*");
    let mut read_table_chars: HashSet<&'static str> = HashSet::new();
    if let Some(read_table) = &read_table {
        if let ExpEnum::HashMap(map) = &read_table.get().data {
            for key in map.keys() {
                read_table_chars.insert(key);
            }
        }
    }
    let read_table_end_char = lookup_expression(&environment, "*read-table-end-char*");
    while next_chars.is_some() {
        let (mut ch, mut peek_ch) = next_chars.unwrap();

        // Consume leading whitespace.
        while is_whitespace(&ch) {
            if ch == "\n" {
                environment.reader_state.as_mut().unwrap().line += 1;
                environment.reader_state.as_mut().unwrap().column = 0;
            } else {
                environment.reader_state.as_mut().unwrap().column += 1;
            }
            if let Some((tch, pch)) = next2(&mut chars) {
                ch = tch;
                peek_ch = pch;
            } else {
                return Ok((false, chars));
            };
        }

        if ch == "\n" {
            environment.reader_state.as_mut().unwrap().line += 1;
            environment.reader_state.as_mut().unwrap().column = 0;
        } else {
            environment.reader_state.as_mut().unwrap().column += 1;
        }
        let mut do_match = true;
        if read_table_chars.contains(&*ch) {
            let mut end_ch = None;
            if let Some(read_table_end_char) = &read_table_end_char {
                if let ExpEnum::HashMap(map) = &read_table_end_char.get().data {
                    if map.contains_key(&*ch) {
                        if let ExpEnum::Char(ch) = &map.get(&*ch).unwrap().get().data {
                            end_ch = Some(cow_to_ref(environment, &ch));
                        }
                    }
                }
            }
            if let Some(read_table) = &read_table {
                if let ExpEnum::HashMap(map) = &read_table.get().data {
                    if map.contains_key(&*ch) {
                        if let ExpEnum::Symbol(s, _) = map.get(&*ch).unwrap().get().data {
                            chars = prep_reader_macro(environment, chars, stack, s, &ch, end_ch)?;
                            do_match = false;
                        }
                    }
                }
            }
        }
        if do_match {
            match &*ch {
                "\"" => {
                    let read_str = match read_string(
                        &mut chars,
                        buffer,
                        &mut environment.reader_state.as_mut().unwrap(),
                    ) {
                        Ok(s) => s,
                        Err(e) => return Err((e, chars)),
                    };
                    if let Err(e) = push_stack(
                        stack,
                        read_str,
                        environment.reader_state.as_ref().unwrap().line,
                        environment.reader_state.as_ref().unwrap().column,
                    ) {
                        return Err((e, chars));
                    }
                }
                "'" => {
                    let mut quoted = Vec::<Expression>::new();
                    quoted.push(Expression::alloc_data(ExpEnum::Symbol(
                        environment.interner.intern("quote"),
                        SymLoc::None,
                    )));
                    stack.push(List {
                        list_type: ListType::List,
                        vec: quoted,
                    });
                    let save_line = environment.reader_state.as_ref().unwrap().line;
                    let save_col = environment.reader_state.as_ref().unwrap().column;
                    let (_, ichars) = read_inner(environment, chars, stack, buffer, in_back_quote)?;
                    chars = ichars;
                    if let Err(e) = close_list(
                        stack,
                        get_meta(
                            environment.reader_state.as_ref().unwrap().file_name,
                            save_line,
                            save_col,
                        ),
                    ) {
                        return Err((e, chars));
                    }
                }
                "`" => {
                    let mut quoted = Vec::<Expression>::new();
                    if in_back_quote {
                        quoted.push(Expression::alloc_data(ExpEnum::Symbol(
                            environment.interner.intern("quote"),
                            SymLoc::None,
                        )));
                    } else {
                        quoted.push(Expression::alloc_data(ExpEnum::Symbol(
                            environment.interner.intern("back-quote"),
                            SymLoc::None,
                        )));
                    }
                    stack.push(List {
                        list_type: ListType::List,
                        vec: quoted,
                    });
                    let save_line = environment.reader_state.as_ref().unwrap().line;
                    let save_col = environment.reader_state.as_ref().unwrap().column;
                    let (_, ichars) = read_inner(environment, chars, stack, buffer, true)?;
                    chars = ichars;
                    if let Err(e) = close_list(
                        stack,
                        get_meta(
                            environment.reader_state.as_ref().unwrap().file_name,
                            save_line,
                            save_col,
                        ),
                    ) {
                        return Err((e, chars));
                    }
                }
                "," if in_back_quote => {
                    read_next = true; // , always needs the symbol after
                    if peek_ch == "@" {
                        chars.next();
                        if let Err(e) = push_stack(
                            stack,
                            Expression::alloc_data(ExpEnum::Symbol(
                                environment.interner.intern(",@"),
                                SymLoc::None,
                            )),
                            environment.reader_state.as_ref().unwrap().line,
                            environment.reader_state.as_ref().unwrap().column,
                        ) {
                            return Err((e, chars));
                        }
                    } else if let Err(e) = push_stack(
                        stack,
                        Expression::alloc_data(ExpEnum::Symbol(
                            environment.interner.intern(","),
                            SymLoc::None,
                        )),
                        environment.reader_state.as_ref().unwrap().line,
                        environment.reader_state.as_ref().unwrap().column,
                    ) {
                        return Err((e, chars));
                    }
                }
                "#" => {
                    chars.next();
                    match &*peek_ch {
                        "|" => consume_block_comment(
                            &mut chars,
                            &mut environment.reader_state.as_mut().unwrap(),
                        ),
                        "\\" => {
                            buffer.clear();
                            read_symbol(
                                buffer,
                                &mut chars,
                                &mut environment.reader_state.as_mut().unwrap(),
                                true,
                                in_back_quote,
                                false,
                            );
                            let do_ch = match do_char(environment, buffer) {
                                Ok(ch) => ch,
                                Err(e) => return Err((e, chars)),
                            };
                            if let Err(e) = push_stack(
                                stack,
                                do_ch,
                                environment.reader_state.as_ref().unwrap().line,
                                environment.reader_state.as_ref().unwrap().column,
                            ) {
                                return Err((e, chars));
                            }
                        }
                        "<" => {
                            let reason = format!(
                                "Found an unreadable token: line {}, col: {}",
                                environment.reader_state.as_ref().unwrap().line,
                                environment.reader_state.as_ref().unwrap().column
                            );
                            return Err((ReadError { reason }, chars));
                        }
                        "(" => {
                            level += 1;
                            stack.push(List {
                                list_type: ListType::Vector,
                                vec: Vec::<Expression>::new(),
                            });
                        }
                        "t" => {
                            if let Err(e) = push_stack(
                                stack,
                                Expression::alloc_data(ExpEnum::True),
                                environment.reader_state.as_ref().unwrap().line,
                                environment.reader_state.as_ref().unwrap().column,
                            ) {
                                return Err((e, chars));
                            }
                        }
                        "." => {
                            chars = prep_reader_macro(
                                environment,
                                chars,
                                stack,
                                "reader-macro-dot",
                                ".",
                                None,
                            )?;
                        }
                        // Read an octal int
                        "o" => {
                            chars =
                                read_num_radix(environment, chars, stack, buffer, in_back_quote, 8)?
                        }
                        // Read a hex int
                        "x" => {
                            chars = read_num_radix(
                                environment,
                                chars,
                                stack,
                                buffer,
                                in_back_quote,
                                16,
                            )?
                        }
                        // Read a binary int
                        "b" => {
                            chars =
                                read_num_radix(environment, chars, stack, buffer, in_back_quote, 2)?
                        }
                        _ => {
                            let reason = format!(
                                "Found # with invalid char {}: line {}, col: {}",
                                peek_ch,
                                environment.reader_state.as_ref().unwrap().line,
                                environment.reader_state.as_ref().unwrap().column
                            );
                            return Err((ReadError { reason }, chars));
                        }
                    }
                }
                "(" => {
                    level += 1;
                    line_stack.push((
                        environment.reader_state.as_ref().unwrap().line,
                        environment.reader_state.as_ref().unwrap().column,
                    ));
                    stack.push(List {
                        list_type: ListType::List,
                        vec: Vec::<Expression>::new(),
                    });
                }
                ")" => {
                    if level <= 0 {
                        return Err((
                            ReadError {
                                reason: "Unexpected `)`".to_string(),
                            },
                            chars,
                        ));
                    }
                    level -= 1;
                    let (line, column) = line_stack.pop().unwrap_or((0, 0));
                    if let Err(e) = close_list(
                        stack,
                        get_meta(
                            environment.reader_state.as_ref().unwrap().file_name,
                            line,
                            column,
                        ),
                    ) {
                        return Err((e, chars));
                    }
                }
                ";" => {
                    consume_line_comment(
                        &mut chars,
                        &mut environment.reader_state.as_mut().unwrap(),
                    );
                }
                _ => {
                    buffer.clear();
                    buffer.push_str(&ch);
                    let is_number = read_symbol(
                        buffer,
                        &mut chars,
                        &mut environment.reader_state.as_mut().unwrap(),
                        false,
                        in_back_quote,
                        false,
                    );
                    if let Err(e) = push_stack(
                        stack,
                        do_atom(environment, buffer, is_number),
                        environment.reader_state.as_ref().unwrap().line,
                        environment.reader_state.as_ref().unwrap().column,
                    ) {
                        return Err((e, chars));
                    }
                }
            }
        }
        if level == 0 && !read_next {
            consume_trailing_whitespace(environment, &mut chars);
            return Ok((true, chars));
        }
        read_next = false;
        next_chars = next2(&mut chars);
    }
    if level != 0 {
        Err((
            ReadError {
                reason: "Unclosed list(s)".to_string(),
            },
            chars,
        ))
    } else {
        consume_trailing_whitespace(environment, &mut chars);
        Ok((false, chars))
    }
}

fn stack_to_exp(
    mut stack: &mut Vec<List>,
    exp_meta: Option<ExpMeta>,
    always_wrap: bool,
    list_only: bool,
) -> Result<Expression, ReadError> {
    close_list(&mut stack, exp_meta)?;
    if stack.len() > 1 {
        Err(ReadError {
            reason: "WTF?".to_string(),
        })
    } else {
        match stack.pop() {
            Some(mut v) => {
                if v.vec.is_empty() {
                    Err(ReadError {
                        reason: "Empty results".to_string(),
                    })
                } else if v.vec.len() == 1 && !always_wrap {
                    let exp: Expression = v.vec.pop().unwrap();
                    if list_only {
                        // If we only have one thing and it is a vector or list then
                        // remove the outer list that was added (unless always_wrap
                        // is set).
                        let exp_d = &exp.get().data;
                        match exp_d {
                            ExpEnum::Vector(_) => Ok(exp.clone()),
                            ExpEnum::Pair(_, _) => Ok(exp.clone()),
                            ExpEnum::Nil => Ok(exp.clone()),
                            _ => {
                                v.vec.push(exp.clone());
                                Ok(Expression::with_list_meta(v.vec, exp_meta))
                            }
                        }
                    } else {
                        Ok(exp)
                    }
                } else {
                    Ok(Expression::with_list_meta(v.vec, exp_meta))
                }
            }
            None => Err(ReadError {
                reason: "WTF, Empty results".to_string(),
            }),
        }
    }
}

fn read2(
    environment: &mut Environment,
    text: &str,
    always_wrap: bool,
    file_name: Option<&'static str>,
    list_only: bool,
) -> Result<Expression, ReadError> {
    let clear_state = if environment.reader_state.is_none() {
        environment.reader_state = Some(ReaderState {
            file_name,
            column: 0,
            line: 1,
            end_ch: None,
        });
        true
    } else {
        false
    };
    let mut buffer = String::new();

    let mut stack: Vec<List> = Vec::new();
    stack.push(List {
        list_type: ListType::Vector,
        vec: Vec::<Expression>::new(),
    });
    // Do this so the chars iterator has a static lifetime.  Should be ok since both the string
    // reference and iterator go away at the end of this function.
    let ntext = unsafe { &*(text as *const str) };
    let mut chars: CharIter = Box::new(
        UnicodeSegmentation::graphemes(ntext, true)
            .map(|s| Cow::Borrowed(s))
            .peekable(),
    );
    if text.starts_with("#!") {
        // Work with shebanged scripts.
        consume_line_comment(&mut chars, &mut environment.reader_state.as_mut().unwrap());
    }
    let mut cont = true;
    while cont {
        let (icont, ichars) = match read_inner(environment, chars, &mut stack, &mut buffer, false) {
            Ok(icont) => icont,
            Err((err, _)) => {
                if clear_state {
                    environment.reader_state = None;
                }
                return Err(err);
            }
        };
        cont = icont;
        chars = ichars;
    }
    if chars.next().is_some() {
        if clear_state {
            environment.reader_state = None;
        }
        let reason = format!(
            "Premature end (to many ')'?) line: {}, column: {}",
            environment.reader_state.as_ref().unwrap().line,
            environment.reader_state.as_ref().unwrap().column
        );
        return Err(ReadError { reason });
    }
    let exp_meta = get_meta(environment.reader_state.as_ref().unwrap().file_name, 0, 0);
    let res = stack_to_exp(&mut stack, exp_meta, always_wrap, list_only);
    if clear_state {
        environment.reader_state = None;
    }
    res
}

pub fn read_form(
    environment: &mut Environment,
    chars: CharIter,
) -> Result<(Expression, CharIter), (ReadError, CharIter)> {
    let clear_state = if environment.reader_state.is_none() {
        environment.reader_state = Some(ReaderState {
            file_name: None,
            column: 0,
            line: 1,
            end_ch: None,
        });
        true
    } else {
        false
    };
    let mut buffer = String::new();
    let mut stack: Vec<List> = Vec::new();
    stack.push(List {
        list_type: ListType::Vector,
        vec: Vec::<Expression>::new(),
    });
    let mut ichars = chars;
    let mut icont = true;
    while icont && stack.len() == 1 && stack[0].vec.is_empty() {
        let (tcont, tchars) = match read_inner(environment, ichars, &mut stack, &mut buffer, false)
        {
            Ok((cont, ichars)) => (cont, ichars),
            Err((e, ichars)) => {
                if clear_state {
                    environment.reader_state = None;
                }
                return Err((e, ichars));
            }
        };
        icont = tcont;
        ichars = tchars;
    }
    let exp_meta = get_meta(
        environment.reader_state.as_ref().unwrap().file_name,
        environment.reader_state.as_ref().unwrap().line,
        environment.reader_state.as_ref().unwrap().column,
    );
    if clear_state {
        environment.reader_state = None;
    }
    match stack_to_exp(&mut stack, exp_meta, false, false) {
        Ok(exp) => Ok((exp, ichars)),
        Err(e) => Err((e, ichars)),
    }
}

pub fn read(
    environment: &mut Environment,
    text: &str,
    name: Option<&'static str>,
    list_only: bool,
) -> Result<Expression, ReadError> {
    read2(environment, text, false, name, list_only)
}

// Read the text but always wrap in an outer list even if text is one list.
// Useful for loading scripts.
pub fn read_list_wrap(
    environment: &mut Environment,
    text: &str,
    name: Option<&'static str>,
) -> Result<Expression, ReadError> {
    read2(environment, text, true, name, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    use crate::builtins_util::is_proper_list;

    fn to_strs(output: &mut Vec<String>, exp: &Expression) {
        match &exp.get().data {
            ExpEnum::Vector(list) => {
                output.push("#(".to_string());
                for exp in list.iter() {
                    to_strs(output, &exp);
                }
                output.push(")".to_string());
            }
            ExpEnum::Pair(e1, e2) => {
                if is_proper_list(&exp) {
                    output.push("(".to_string());
                    for p in exp.iter() {
                        to_strs(output, &p);
                    }
                    output.push(")".to_string());
                } else {
                    output.push("(".to_string());
                    to_strs(output, &e1);
                    output.push(".".to_string());
                    to_strs(output, &e2);
                    output.push(")".to_string());
                }
            }
            ExpEnum::Nil => output.push("nil".to_string()),
            _ => {
                output.push(format!("{}:{}", exp.display_type(), exp.to_string()));
            }
        }
    }

    fn tokenize(
        environment: &mut Environment,
        input: &str,
        name: Option<&'static str>,
    ) -> Vec<String> {
        let exp = read(environment, input, name, false);
        let mut tokens = Vec::new();
        if let Ok(exp) = exp {
            to_strs(&mut tokens, &exp);
        } else {
            println!("{:?}", exp);
            assert!(false);
        }
        tokens
    }

    fn tokenize_wrap(
        environment: &mut Environment,
        input: &str,
        name: Option<&'static str>,
    ) -> Vec<String> {
        let exp = read_list_wrap(environment, input, name);
        let mut tokens = Vec::new();
        if let Ok(exp) = exp {
            to_strs(&mut tokens, &exp);
        } else {
            assert!(false);
        }
        tokens
    }

    fn build_def_env() -> Environment {
        let mut environment = build_default_environment(Arc::new(AtomicBool::new(false)));
        environment.reader_state = Some(ReaderState {
            line: 0,
            column: 0,
            file_name: None,
            end_ch: None,
        });
        environment
    }

    #[test]
    fn test_tokenize() {
        let mut environment = build_def_env();
        let tokens = tokenize(&mut environment, "one two three \"four\" 5 6", None);
        assert!(tokens.len() == 8);
        assert!(tokens[0] == "#(");
        assert!(tokens[1] == "Symbol:one");
        assert!(tokens[2] == "Symbol:two");
        assert!(tokens[3] == "Symbol:three");
        assert!(tokens[4] == "String:\"four\"");
        assert!(tokens[5] == "Int:5");
        assert!(tokens[6] == "Int:6");
        assert!(tokens[7] == ")");
        let tokens = tokenize(&mut environment, "(1 2 3)", None);
        assert!(tokens.len() == 5);
        assert!(tokens[0] == "(");
        assert!(tokens[1] == "Int:1");
        assert!(tokens[2] == "Int:2");
        assert!(tokens[3] == "Int:3");
        assert!(tokens[4] == ")");
        let tokens = tokenize(&mut environment, "  (  1    2\t3   )  ", None);
        assert!(tokens.len() == 5);
        assert!(tokens[0] == "(");
        assert!(tokens[1] == "Int:1");
        assert!(tokens[2] == "Int:2");
        assert!(tokens[3] == "Int:3");
        assert!(tokens[4] == ")");
        let tokens = tokenize(&mut environment, "#(#\\A 2 3)", None);
        assert!(tokens.len() == 5);
        assert!(tokens[0] == "#(");
        assert!(tokens[1] == "Char:#\\A");
        assert!(tokens[2] == "Int:2");
        assert!(tokens[3] == "Int:3");
        assert!(tokens[4] == ")");
        let tokens = tokenize(&mut environment, "#(#\\  2 3)", None);
        assert!(tokens.len() == 5);
        assert!(tokens[0] == "#(");
        assert!(tokens[1] == "Char:#\\ ");
        assert!(tokens[2] == "Int:2");
        assert!(tokens[3] == "Int:3");
        assert!(tokens[4] == ")");
        let tokens = tokenize(&mut environment, "'((1 2 (3)))", None);
        assert!(tokens.len() == 12);
        assert!(tokens[0] == "(");
        assert!(tokens[1] == "Symbol:quote");
        assert!(tokens[2] == "(");
        assert!(tokens[3] == "(");
        assert!(tokens[4] == "Int:1");
        assert!(tokens[5] == "Int:2");
        assert!(tokens[6] == "(");
        assert!(tokens[7] == "Int:3");
        assert!(tokens[8] == ")");
        assert!(tokens[9] == ")");
        assert!(tokens[10] == ")");
        assert!(tokens[11] == ")");
        let tokens = tokenize(&mut environment, "(length \"12345\")", None);
        assert!(tokens.len() == 4);
        assert!(tokens[0] == "(");
        assert!(tokens[1] == "Symbol:length");
        assert!(tokens[2] == "String:\"12345\"");
        assert!(tokens[3] == ")");
        let tokens = tokenize(&mut environment, "(length \"12345Σ\")", None);
        assert!(tokens.len() == 4);
        assert!(tokens[0] == "(");
        assert!(tokens[1] == "Symbol:length");
        assert!(tokens[2] == "String:\"12345Σ\"");
        assert!(tokens[3] == ")");
    }

    #[test]
    fn test_quotes() {
        let mut environment = build_def_env();
        let tokens = tokenize(&mut environment, "'(1 2 3)", None);
        assert!(tokens.len() == 8);
        assert!(tokens[0] == "(");
        assert!(tokens[1] == "Symbol:quote");
        assert!(tokens[2] == "(");
        assert!(tokens[3] == "Int:1");
        assert!(tokens[4] == "Int:2");
        assert!(tokens[5] == "Int:3");
        assert!(tokens[6] == ")");
        assert!(tokens[7] == ")");
        let tokens = tokenize(&mut environment, "'(1 2 ,3)", None);
        assert!(tokens.len() == 8);
        assert!(tokens[0] == "(");
        assert!(tokens[1] == "Symbol:quote");
        assert!(tokens[2] == "(");
        assert!(tokens[3] == "Int:1");
        assert!(tokens[4] == "Int:2");
        assert!(tokens[5] == "Symbol:,3");
        assert!(tokens[6] == ")");
        assert!(tokens[7] == ")");
        let tokens = tokenize(&mut environment, "'(1 2 ,@3)", None);
        assert!(tokens.len() == 8);
        assert!(tokens[0] == "(");
        assert!(tokens[1] == "Symbol:quote");
        assert!(tokens[2] == "(");
        assert!(tokens[3] == "Int:1");
        assert!(tokens[4] == "Int:2");
        assert!(tokens[5] == "Symbol:,@3");
        assert!(tokens[6] == ")");
        assert!(tokens[7] == ")");
        let tokens = tokenize(&mut environment, "`(1 2 ,3)", None);
        assert!(tokens.len() == 9);
        assert!(tokens[0] == "(");
        assert!(tokens[1] == "Symbol:back-quote");
        assert!(tokens[2] == "(");
        assert!(tokens[3] == "Int:1");
        assert!(tokens[4] == "Int:2");
        assert!(tokens[5] == "Symbol:,");
        assert!(tokens[6] == "Int:3");
        assert!(tokens[7] == ")");
        assert!(tokens[8] == ")");
        let tokens = tokenize(&mut environment, "`(1 2 ,@3)", None);
        assert!(tokens.len() == 9);
        assert!(tokens[0] == "(");
        assert!(tokens[1] == "Symbol:back-quote");
        assert!(tokens[2] == "(");
        assert!(tokens[3] == "Int:1");
        assert!(tokens[4] == "Int:2");
        assert!(tokens[5] == "Symbol:,@");
        assert!(tokens[6] == "Int:3");
        assert!(tokens[7] == ")");
        assert!(tokens[8] == ")");
        let tokens = tokenize(&mut environment, "`(1 `2 ,@3)", None);
        assert!(tokens.len() == 12);
        assert!(tokens[0] == "(");
        assert!(tokens[1] == "Symbol:back-quote");
        assert!(tokens[2] == "(");
        assert!(tokens[3] == "Int:1");
        assert!(tokens[4] == "(");
        assert!(tokens[5] == "Symbol:quote");
        assert!(tokens[6] == "Int:2");
        assert!(tokens[7] == ")");
        assert!(tokens[8] == "Symbol:,@");
        assert!(tokens[9] == "Int:3");
        assert!(tokens[10] == ")");
        assert!(tokens[11] == ")");
        let tokens = tokenize(&mut environment, "`(1 `(2 ,x) ,@3)", None);
        assert!(tokens.len() == 16);
        assert!(tokens[0] == "(");
        assert!(tokens[1] == "Symbol:back-quote");
        assert!(tokens[2] == "(");
        assert!(tokens[3] == "Int:1");
        assert!(tokens[4] == "(");
        assert!(tokens[5] == "Symbol:quote");
        assert!(tokens[6] == "(");
        assert!(tokens[7] == "Int:2");
        assert!(tokens[8] == "Symbol:,");
        assert!(tokens[9] == "Symbol:x");
        assert!(tokens[10] == ")");
        assert!(tokens[11] == ")");
        assert!(tokens[12] == "Symbol:,@");
        assert!(tokens[13] == "Int:3");
        assert!(tokens[14] == ")");
        assert!(tokens[15] == ")");
    }

    #[test]
    fn test_types() {
        let mut environment = build_def_env();
        let tokens = tokenize(
            &mut environment,
            "(one 2 3.0 \"four\" #\\B #t nil 3.5 ())",
            None,
        );
        assert!(tokens.len() == 11);
        assert!(tokens[0] == "(");
        assert!(tokens[1] == "Symbol:one");
        assert!(tokens[2] == "Int:2");
        assert!(tokens[3] == "Float:3");
        assert!(tokens[4] == "String:\"four\"");
        assert!(tokens[5] == "Char:#\\B");
        assert!(tokens[6] == "True:true");
        assert!(tokens[7] == "nil");
        assert!(tokens[8] == "Float:3.5");
        assert!(tokens[9] == "nil");
        assert!(tokens[10] == ")");

        let tokens = tokenize(
            &mut environment,
            "#(one 2 3.0 \"four\" #\\B #t nil 3.5 ())",
            None,
        );
        assert!(tokens.len() == 11);
        assert!(tokens[0] == "#(");
        assert!(tokens[1] == "Symbol:one");
        assert!(tokens[2] == "Int:2");
        assert!(tokens[3] == "Float:3");
        assert!(tokens[4] == "String:\"four\"");
        assert!(tokens[5] == "Char:#\\B");
        assert!(tokens[6] == "True:true");
        assert!(tokens[7] == "nil");
        assert!(tokens[8] == "Float:3.5");
        assert!(tokens[9] == "nil");
        assert!(tokens[10] == ")");

        let tokens = tokenize(
            &mut environment,
            "one 2 3.0 \"four\" #\\B #t nil 3.5 ()",
            None,
        );
        assert!(tokens.len() == 11);
        assert!(tokens[0] == "#(");
        assert!(tokens[1] == "Symbol:one");
        assert!(tokens[2] == "Int:2");
        assert!(tokens[3] == "Float:3");
        assert!(tokens[4] == "String:\"four\"");
        assert!(tokens[5] == "Char:#\\B");
        assert!(tokens[6] == "True:true");
        assert!(tokens[7] == "nil");
        assert!(tokens[8] == "Float:3.5");
        assert!(tokens[9] == "nil");
        assert!(tokens[10] == ")");
    }

    #[test]
    fn test_wrap() {
        let mut environment = build_def_env();
        let tokens = tokenize(&mut environment, "(1 2 3)", None);
        assert!(tokens.len() == 5);
        assert!(tokens[0] == "(");
        assert!(tokens[1] == "Int:1");
        assert!(tokens[2] == "Int:2");
        assert!(tokens[3] == "Int:3");
        assert!(tokens[4] == ")");
        let tokens = tokenize_wrap(&mut environment, "(1 2 3)", None);
        assert!(tokens.len() == 7);
        assert!(tokens[0] == "#(");
        assert!(tokens[1] == "(");
        assert!(tokens[2] == "Int:1");
        assert!(tokens[3] == "Int:2");
        assert!(tokens[4] == "Int:3");
        assert!(tokens[5] == ")");
        assert!(tokens[6] == ")");

        let tokens = tokenize(&mut environment, "1 2 3", None);
        assert!(tokens.len() == 5);
        assert!(tokens[0] == "#(");
        assert!(tokens[1] == "Int:1");
        assert!(tokens[2] == "Int:2");
        assert!(tokens[3] == "Int:3");
        assert!(tokens[4] == ")");
        let tokens = tokenize_wrap(&mut environment, "1 2 3", None);
        assert!(tokens.len() == 5);
        assert!(tokens[0] == "#(");
        assert!(tokens[1] == "Int:1");
        assert!(tokens[2] == "Int:2");
        assert!(tokens[3] == "Int:3");
        assert!(tokens[4] == ")");

        let tokens = tokenize(&mut environment, "(1 2 3) (4 5 6)", None);
        assert!(tokens.len() == 12);
        assert!(tokens[0] == "#(");
        assert!(tokens[1] == "(");
        assert!(tokens[2] == "Int:1");
        assert!(tokens[3] == "Int:2");
        assert!(tokens[4] == "Int:3");
        assert!(tokens[5] == ")");
        assert!(tokens[6] == "(");
        assert!(tokens[7] == "Int:4");
        assert!(tokens[8] == "Int:5");
        assert!(tokens[9] == "Int:6");
        assert!(tokens[10] == ")");
        assert!(tokens[11] == ")");
        let tokens = tokenize_wrap(&mut environment, "(1 2 3) (4 5 6)", None);
        assert!(tokens.len() == 12);
        assert!(tokens[0] == "#(");
        assert!(tokens[1] == "(");
        assert!(tokens[2] == "Int:1");
        assert!(tokens[3] == "Int:2");
        assert!(tokens[4] == "Int:3");
        assert!(tokens[5] == ")");
        assert!(tokens[6] == "(");
        assert!(tokens[7] == "Int:4");
        assert!(tokens[8] == "Int:5");
        assert!(tokens[9] == "Int:6");
        assert!(tokens[10] == ")");
        assert!(tokens[11] == ")");

        let tokens = tokenize(&mut environment, "'(1 2 3)", None);
        assert!(tokens.len() == 8);
        assert!(tokens[0] == "(");
        assert!(tokens[1] == "Symbol:quote");
        assert!(tokens[2] == "(");
        assert!(tokens[3] == "Int:1");
        assert!(tokens[4] == "Int:2");
        assert!(tokens[5] == "Int:3");
        assert!(tokens[6] == ")");
        assert!(tokens[7] == ")");
        let tokens = tokenize_wrap(&mut environment, "'(1 2 3)", None);
        assert!(tokens.len() == 10);
        assert!(tokens[0] == "#(");
        assert!(tokens[1] == "(");
        assert!(tokens[2] == "Symbol:quote");
        assert!(tokens[3] == "(");
        assert!(tokens[4] == "Int:1");
        assert!(tokens[5] == "Int:2");
        assert!(tokens[6] == "Int:3");
        assert!(tokens[7] == ")");
        assert!(tokens[8] == ")");
        assert!(tokens[9] == ")");

        let tokens = tokenize(&mut environment, "nil", None);
        assert!(tokens.len() == 1);
        assert!(tokens[0] == "nil");
        let tokens = tokenize(&mut environment, "()", None);
        assert!(tokens.len() == 1);
        assert!(tokens[0] == "nil");
        let tokens = tokenize_wrap(&mut environment, "nil", None);
        assert!(tokens.len() == 3);
        assert!(tokens[0] == "#(");
        assert!(tokens[1] == "nil");
        assert!(tokens[2] == ")");
        let tokens = tokenize_wrap(&mut environment, "()", None);
        assert!(tokens.len() == 3);
        assert!(tokens[0] == "#(");
        assert!(tokens[1] == "nil");
        assert!(tokens[2] == ")");
    }

    #[test]
    fn test_tok_strings() {
        let mut environment = build_def_env();
        let input =
            "\"on\\te\\ntwo\" two \"th\\rree\" \"fo\\\"u\\\\r\" 5 6 \"slash\\x2fx\\x2F\\x3a\\x3b\"";
        let tokens = tokenize(&mut environment, input, None);
        assert!(tokens.len() == 9);
        assert!(tokens[0] == "#(");
        assert!(tokens[1] == "String:\"on\te\ntwo\"");
        assert!(tokens[2] == "Symbol:two");
        assert!(tokens[3] == "String:\"th\rree\"");
        assert!(tokens[4] == "String:\"fo\"u\\r\"");
        assert!(tokens[5] == "Int:5");
        assert!(tokens[6] == "Int:6");
        assert!(tokens[7] == "String:\"slash/x/:;\"");
        assert!(tokens[8] == ")");

        let input =
            "\"\\u{03bb} two \" \"\\x20 \\u{03BB} end\" \"fo\\\"u\\\\r\" 5 6 \"slash\\x2fx\\x2F\\x3a\\x3b\"";
        let tokens = tokenize(&mut environment, input, None);
        assert!(tokens.len() == 8);
        assert!(tokens[0] == "#(");
        assert!(tokens[1] == "String:\"\u{03bb} two \"");
        assert!(tokens[2] == "String:\"  λ end\"");
        assert!(tokens[3] == "String:\"fo\"u\\r\"");
        assert!(tokens[4] == "Int:5");
        assert!(tokens[5] == "Int:6");
        assert!(tokens[6] == "String:\"slash/x/:;\"");
        assert!(tokens[7] == ")");

        let input =
            "\"\\u03bb two \" \"\\x20 \\u03BB \nend\" \"fo\\\"u\\\\r\" 5 6 \"slash\\x2fx\\x2F\\x3a\\x3b\"";
        let tokens = tokenize(&mut environment, input, None);
        assert!(tokens.len() == 8);
        assert!(tokens[0] == "#(");
        assert!(tokens[1] == "String:\"\u{03bb} two \"");
        assert!(tokens[2] == "String:\"  λ \nend\"");
        assert!(tokens[3] == "String:\"fo\"u\\r\"");
        assert!(tokens[4] == "Int:5");
        assert!(tokens[5] == "Int:6");
        assert!(tokens[6] == "String:\"slash/x/:;\"");
        assert!(tokens[7] == ")");
    }

    #[test]
    fn test_tok_chars() {
        let mut environment = build_def_env();
        let input = "#\\x #\\X #\\x20 #\\u03bb #\\u{03BB} #\\u03bb";
        let tokens = tokenize(&mut environment, input, None);
        assert!(tokens.len() == 8);
        assert!(tokens[0] == "#(");
        assert!(tokens[1] == "Char:#\\x");
        assert!(tokens[2] == "Char:#\\X");
        assert!(tokens[3] == "Char:#\\ ");
        assert!(tokens[4] == "Char:#\\λ");
        assert!(tokens[5] == "Char:#\\\u{03bb}");
        assert!(tokens[6] == "Char:#\\λ");
        assert!(tokens[7] == ")");
    }
}
