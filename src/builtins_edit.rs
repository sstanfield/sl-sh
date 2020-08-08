use std::collections::HashMap;
use std::env;
use std::hash::BuildHasher;
use std::io::{self, ErrorKind};

use liner::{keymap, Buffer, ColorClosure, Context, Prompt};

use crate::builtins_util::*;
use crate::completions::*;
use crate::environment::*;
use crate::eval::*;
use crate::interner::*;
use crate::types::*;

fn load_repl_settings(repl_settings: &Expression) -> ReplSettings {
    let mut ret = ReplSettings::default();
    if let ExpEnum::HashMap(repl_settings) = &repl_settings.get().data {
        if let Some(keybindings) = repl_settings.get(":keybindings") {
            let keybindings: Expression = keybindings.into();
            if let ExpEnum::Atom(Atom::Symbol(keybindings)) = &keybindings.get().data {
                match &keybindings[..] {
                    ":vi" => ret.key_bindings = Keys::Vi,
                    ":emacs" => ret.key_bindings = Keys::Emacs,
                    _ => eprintln!("Invalid keybinding setting: {}", keybindings),
                }
            };
        }
        if let Some(max) = repl_settings.get(":max-history") {
            let max: Expression = max.into();
            if let ExpEnum::Atom(Atom::Int(max)) = &max.get().data {
                if *max >= 0 {
                    ret.max_history = *max as usize;
                } else {
                    eprintln!("Max history must be positive: {}", max);
                }
            } else {
                eprintln!("Max history must be a positive integer: {}", max);
            };
        }
        if let Some(vi_esc) = repl_settings.get(":vi_esc_sequence") {
            let vi_esc: Expression = vi_esc.into();
            let mut i = vi_esc.iter();
            if let Some(arg0) = i.next() {
                if let ExpEnum::Atom(Atom::String(keys, _)) = &arg0.get().data {
                    if let Some(arg1) = i.next() {
                        if let ExpEnum::Atom(Atom::Int(ms)) = &arg1.get().data {
                            if keys.len() == 2 {
                                let mut chars = keys.chars();
                                ret.vi_esc_sequence = Some((
                                    chars.next().unwrap(),
                                    chars.next().unwrap(),
                                    *ms as u32,
                                ));
                            } else {
                                eprintln!(":vi_esc_sequence first value should be a string of two characters (two key sequence for escape)");
                            }
                        } else {
                            eprintln!(":vi_esc_sequence second value should be number (ms delay)");
                        }
                    } else {
                        eprintln!(":vi_esc_sequence second value should be number (ms delay)");
                    }
                } else {
                    eprintln!(
                    ":vi_esc_sequence first value should be a string (two key sequence for escape)"
                );
                }
            } else {
                eprintln!(
                    ":vi_esc_sequence first value should be a string (two key sequence for escape)"
                );
            }
        }
        if let Some(prefix) = repl_settings.get(":vi-normal-prompt-prefix") {
            let prefix: Expression = prefix.into();
            if let ExpEnum::Atom(Atom::String(prefix, _)) = &prefix.get().data {
                ret.vi_normal_prompt_prefix = Some(prefix.to_string());
            };
        }
        if let Some(suffix) = repl_settings.get(":vi-normal-prompt-suffix") {
            let suffix: Expression = suffix.into();
            if let ExpEnum::Atom(Atom::String(suffix, _)) = &suffix.get().data {
                ret.vi_normal_prompt_suffix = Some(suffix.to_string());
            };
        }
        if let Some(prefix) = repl_settings.get(":vi-insert-prompt-prefix") {
            let prefix: Expression = prefix.into();
            if let ExpEnum::Atom(Atom::String(prefix, _)) = &prefix.get().data {
                ret.vi_insert_prompt_prefix = Some(prefix.to_string());
            };
        }
        if let Some(suffix) = repl_settings.get(":vi-insert-prompt-suffix") {
            let suffix: Expression = suffix.into();
            if let ExpEnum::Atom(Atom::String(suffix, _)) = &suffix.get().data {
                ret.vi_insert_prompt_suffix = Some(suffix.to_string());
            };
        }
    }
    ret
}

// Like the liner default but make '(' and ')' their own words for cleaner completions.
fn get_liner_words(buf: &Buffer) -> Vec<(usize, usize)> {
    let mut res = Vec::new();

    let mut word_start = None;
    let mut just_had_backslash = false;

    for (i, &c) in buf.chars().enumerate() {
        if c == '\\' {
            just_had_backslash = true;
            continue;
        }

        if let Some(start) = word_start {
            if (c == ' ' || c == '(' || c == ')') && !just_had_backslash {
                res.push((start, i));
                if c == '(' || c == ')' {
                    res.push((i, i + 1));
                }
                word_start = None;
            }
        } else if c == '(' || c == ')' {
            res.push((i, i + 1));
        } else if c != ' ' {
            word_start = Some(i);
        }

        just_had_backslash = false;
    }

    if let Some(start) = word_start {
        res.push((start, buf.num_chars()));
    }

    res
}

fn apply_repl_settings(con: &mut Context, repl_settings: &ReplSettings) {
    let keymap: Box<dyn keymap::KeyMap> = match repl_settings.key_bindings {
        Keys::Vi => {
            let mut vi = keymap::Vi::new();
            if let Some((ch1, ch2, timeout)) = repl_settings.vi_esc_sequence {
                vi.set_esc_sequence(ch1, ch2, timeout);
            }
            vi.set_normal_prompt_prefix(repl_settings.vi_normal_prompt_prefix.clone());
            vi.set_normal_prompt_suffix(repl_settings.vi_normal_prompt_suffix.clone());
            vi.set_insert_prompt_prefix(repl_settings.vi_insert_prompt_prefix.clone());
            vi.set_insert_prompt_suffix(repl_settings.vi_insert_prompt_suffix.clone());
            Box::new(vi)
        }
        Keys::Emacs => Box::new(keymap::Emacs::new()),
    };
    con.set_keymap(keymap);
    con.history.set_max_history_size(repl_settings.max_history);
}

fn make_con(environment: &mut Environment, history: Option<&str>) -> Context {
    let mut con = Context::new();
    con.set_word_divider(Box::new(get_liner_words));
    let mut home = match env::var("HOME") {
        Ok(val) => val,
        Err(_) => ".".to_string(),
    };
    if home.ends_with('/') {
        home = home[..home.len() - 1].to_string();
    }
    if let Some(history) = history {
        let history_file = if history.starts_with('/') || history.starts_with('.') {
            history.to_string()
        } else {
            format!("{}/.local/share/sl-sh/{}", home, history)
        };
        if let Err(err) = con.history.set_file_name_and_load_history(&history_file) {
            eprintln!(
                "WARNING: Unable to load history file {}: {}",
                history_file, err
            );
        }
    }
    apply_repl_settings(&mut con, &environment.repl_settings);
    con
}

fn get_color_closure(environment: &mut Environment) -> Option<ColorClosure> {
    let line_exp = get_from_namespace(environment, "__line_handler");
    if let Some(exp) = line_exp {
        let fn_exp = exp.exp;
        // This unsafe should be OK because the returned object is used in a call to read_line and
        // dropped after.
        let environment = unsafe { &mut *(environment as *mut Environment) };
        Some(Box::new(move |input: &str| -> String {
            let exp = match &fn_exp.get().data {
                ExpEnum::Atom(Atom::Lambda(_)) => {
                    let mut v = Vec::with_capacity(1);
                    v.push(fn_exp.clone().into());
                    v.push(
                        Expression::alloc_data(ExpEnum::Atom(Atom::String(
                            input.to_string().into(),
                            None,
                        )))
                        .handle_no_root(),
                    );
                    Expression::with_list(v)
                }
                _ => return input.to_string(),
            };
            environment.save_exit_status = false; // Do not overwrite last exit status with line_handler.
            environment.str_ignore_expand = true;
            let res = eval(environment, exp);
            environment.str_ignore_expand = false;
            environment.save_exit_status = true;
            res.unwrap_or_else(|e| {
                Expression::alloc_data(ExpEnum::Atom(Atom::String(
                    format!("ERROR: {}", e).into(),
                    None,
                )))
            })
            .as_string(environment)
            .unwrap_or_else(|_| "ERROR".to_string())
        }))
    } else {
        None
    }
}

pub fn read_prompt(
    environment: &mut Environment,
    prompt: &str,
    history: Option<&str>,
    liner_id: &'static str,
) -> io::Result<String> {
    let repl_settings = get_expression(environment, "*repl-settings*").unwrap();
    let new_repl_settings = load_repl_settings(&repl_settings.exp);
    let mut load_settings = if environment.repl_settings != new_repl_settings {
        environment.repl_settings = new_repl_settings.clone();
        true
    } else {
        false
    };
    let mut con = if liner_id == ":new" {
        load_settings = false;
        make_con(environment, history)
    } else if environment.liners.contains_key(liner_id) {
        environment.liners.remove(liner_id).unwrap()
    } else {
        load_settings = false;
        make_con(environment, history)
    };
    if load_settings {
        apply_repl_settings(&mut con, &new_repl_settings);
    };
    // This unsafe should be OK because the con object this is set into is
    // stored in the environment (or dropped at the end of this function)
    // so environment should out live con.
    let env = unsafe { &mut *(environment as *mut Environment) };
    con.set_completer(Box::new(ShellCompleter::new(env)));
    let result = match con.read_line(Prompt::from(prompt), get_color_closure(environment)) {
        Ok(input) => {
            let input = input.trim();
            /*if history.is_some() {
                if let Err(err) = con.history.push(input) {
                    eprintln!("read-line: Error saving history: {}", err);
                }
            }*/
            Ok(input.into())
        }
        Err(err) => Err(err),
    };
    if liner_id != ":new" {
        environment.liners.insert(liner_id, con);
    };
    result
}

fn builtin_prompt(
    environment: &mut Environment,
    args: &mut dyn Iterator<Item = Expression>,
) -> Result<Expression, LispError> {
    let (liner_id, prompt) = {
        let arg1 = param_eval(environment, args, "prompt")?;
        let arg_d = arg1.get();
        if let ExpEnum::Atom(Atom::Symbol(s)) = arg_d.data {
            (s, param_eval(environment, args, "prompt")?)
        } else {
            drop(arg_d);
            (":new", arg1)
        }
    };
    let h_str;
    let history_file = if let Some(h) = args.next() {
        let hist = eval(environment, h)?;
        let hist_d = hist.get();
        if let ExpEnum::Atom(Atom::String(s, _)) = &hist_d.data {
            h_str = match expand_tilde(s) {
                Some(p) => p,
                None => s.to_string(),
            };
            Some(&h_str[..])
        } else {
            return Err(LispError::new(
                "prompt: history file (if provided) must be a string.",
            ));
        }
    } else {
        None
    };
    params_done(args, "prompt")?;
    let prompt_d = prompt.get();
    if let ExpEnum::Atom(Atom::String(s, _)) = &prompt_d.data {
        return match read_prompt(environment, s, history_file, liner_id) {
            Ok(input) => Ok(Expression::alloc_data(ExpEnum::Atom(Atom::String(
                input.into(),
                None,
            )))),
            Err(err) => match err.kind() {
                ErrorKind::UnexpectedEof => {
                    let input =
                        Expression::alloc_data_h(ExpEnum::Atom(Atom::String("".into(), None)));
                    let error =
                        Expression::alloc_data_h(ExpEnum::Atom(Atom::Symbol(":unexpected-eof")));
                    Ok(Expression::alloc_data(ExpEnum::Values(vec![input, error])))
                }
                ErrorKind::Interrupted => {
                    let input =
                        Expression::alloc_data_h(ExpEnum::Atom(Atom::String("".into(), None)));
                    let error =
                        Expression::alloc_data_h(ExpEnum::Atom(Atom::Symbol(":interrupted")));
                    Ok(Expression::alloc_data(ExpEnum::Values(vec![input, error])))
                }
                _ => {
                    eprintln!("Error on input: {}", err);
                    Err(LispError::new("Unexpected input error!"))
                }
            },
        };
    }
    Err(LispError::new(
        "prompt: requires a prompt string and option history file.",
    ))
}

fn builtin_prompt_history_push(
    environment: &mut Environment,
    args: &mut dyn Iterator<Item = Expression>,
) -> Result<Expression, LispError> {
    let liner_id = {
        let arg = param_eval(environment, args, "prompt-history-push")?;
        let arg_d = arg.get();
        if let ExpEnum::Atom(Atom::Symbol(s)) = arg_d.data {
            s
        } else {
            return Err(LispError::new(
                "prompt-history-push: context id must be a keyword.",
            ));
        }
    };
    let item = {
        let arg = param_eval(environment, args, "prompt-history-push")?;
        let arg_d = arg.get();
        if let ExpEnum::Atom(Atom::String(s, _)) = &arg_d.data {
            s.to_string()
        } else {
            return Err(LispError::new(
                "prompt-history-push: history item must be a string.",
            ));
        }
    };
    params_done(args, "prompt-history-push")?;
    let mut con = if environment.liners.contains_key(liner_id) {
        environment.liners.remove(liner_id).unwrap()
    } else {
        return Err(LispError::new("prompt-history-push: context id not found."));
    };
    let result = if let Err(err) = con.history.push(item) {
        eprintln!("Warning: failed to save history: {}", err);
        Ok(Expression::make_nil())
    } else {
        Ok(Expression::make_true())
    };
    environment.liners.insert(liner_id, con);
    result
}

fn builtin_prompt_history_push_throwaway(
    environment: &mut Environment,
    args: &mut dyn Iterator<Item = Expression>,
) -> Result<Expression, LispError> {
    let liner_id = {
        let arg = param_eval(environment, args, "prompt-history-push-throwaway")?;
        let arg_d = arg.get();
        if let ExpEnum::Atom(Atom::Symbol(s)) = arg_d.data {
            s
        } else {
            return Err(LispError::new(
                "prompt-history-push-throwaway: context id must be a keyword.",
            ));
        }
    };
    let item = {
        let arg = param_eval(environment, args, "prompt-history-push-throwaway")?;
        let arg_d = arg.get();
        if let ExpEnum::Atom(Atom::String(s, _)) = &arg_d.data {
            s.to_string()
        } else {
            return Err(LispError::new(
                "prompt-history-push-throwaway: history item must be a string.",
            ));
        }
    };
    params_done(args, "prompt-history-push-throwaway")?;
    let mut con = if environment.liners.contains_key(liner_id) {
        environment.liners.remove(liner_id).unwrap()
    } else {
        return Err(LispError::new(
            "prompt-history-push-throwaway: context id not found.",
        ));
    };
    let result = if let Err(err) = con.history.push_throwaway(item) {
        eprintln!("Warning: failed to save temp history: {}", err);
        Ok(Expression::make_nil())
    } else {
        Ok(Expression::make_true())
    };
    environment.liners.insert(liner_id, con);
    result
}

fn builtin_prompt_history_context(
    environment: &mut Environment,
    args: &mut dyn Iterator<Item = Expression>,
) -> Result<Expression, LispError> {
    let liner_id = {
        let arg = param_eval(environment, args, "prompt-history-context")?;
        let arg_d = arg.get();
        if let ExpEnum::Atom(Atom::Symbol(s)) = arg_d.data {
            s
        } else {
            return Err(LispError::new(
                "prompt-history-context: context id must be a keyword.",
            ));
        }
    };
    let item = {
        let arg = param_eval(environment, args, "prompt-history-context")?;
        let arg_d = arg.get();
        match &arg_d.data {
            ExpEnum::Atom(Atom::String(s, _)) => Some(s.to_string()),
            ExpEnum::Nil => None,
            _ => {
                return Err(LispError::new(
                    "prompt-history-context: history context item must be a string.",
                ))
            }
        }
    };
    params_done(args, "prompt-history-context")?;
    let mut con = if environment.liners.contains_key(liner_id) {
        environment.liners.remove(liner_id).unwrap()
    } else {
        return Ok(Expression::make_nil());
    };
    con.history.set_search_context(item);
    environment.liners.insert(liner_id, con);
    Ok(Expression::make_nil())
}

pub fn add_edit_builtins<S: BuildHasher>(
    interner: &mut Interner,
    data: &mut HashMap<&'static str, Reference, S>,
) {
    let root = interner.intern("root");
    data.insert(
        interner.intern("prompt"),
        Expression::make_function(
            builtin_prompt,
            "Usage: (prompt string) -> string

Starts an interactive prompt (like the repl prompt) with the supplied prompt and
returns the input string.

Section: shell

Example:
;(def 'input-string (prompt \"prompt> \"))
t
",
            root,
        ),
    );
    data.insert(
        interner.intern("prompt-history-push"),
        Expression::make_function(
            builtin_prompt_history_push,
            "Usage: (prompt-history-push :context_id string) -> nil/t

Pushes string onto the history for the prompt context :context_id.
Returns true on success or nil on failure.

Section: shell

Example:
;(prompt-history-push :repl \"Some command\")
t
",
            root,
        ),
    );
    data.insert(
        interner.intern("prompt-history-push-throwaway"),
        Expression::make_function(
            builtin_prompt_history_push_throwaway,
            "Usage: (prompt-history-push-throwaway :context_id string) -> nil/t

Pushes string onto the history for the prompt context :context_id.  A throwaway
item will will only persist until the next command is read (use it to allow
editing of failed commands without them going into history).
Returns true on success or nil on failure.

Section: shell

Example:
;(prompt-history-push-throwaway :repl \"Some broken command\")
t
",
            root,
        ),
    );
    data.insert(
        interner.intern("prompt-history-context"),
        Expression::make_function(
            builtin_prompt_history_context,
            "Usage: (prompt-history-context :context_id context-string) -> nil

Sets the history context for searches.  Usually the current path but can be any
string.  Pass nil to set it to nothing.

Section: shell

Example:
;(prompt-history-context :repl \"/home\")
t
",
            root,
        ),
    );
}
