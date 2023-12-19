//! TODO PC need explanation for the emulation for TryFrom/TryInto/AsRef/AsMut
//! My notes:
//! #. To convert a slosh &Value to an owned type implement `impl SlFrom<&Value> for OwnedType`,
//!     this allows rust native functions annotated with the bridge macro to receive normal
//!     rust types.
//! #. To convert a slosh &Value to a reference type implement `impl SlAsRef<&Value> for RefType`.
//! #. To convert a slosh &Value to a mutable reference type implement `impl SlAsMut<&Value> for MutRefType`.
//! #. To convert some rust type back to a value that the rust native function
//!     annotated by the bridge macro returns implement `impl SlFrom<&Value> for RustType`.
//!     TODO PC blanket impl so impl `SlFrom<Value>` works, and taking a ref isn't required?
//! #. To avoid allocations when converting a slosh &Value back to a rust type that was mutated
//!     don't return anything. If it is necessary for the API to return some value,
//!     TODO PC annotated or liftime? AKA [the extant value problem]
//!
//!
//! ## rosetta stone for bridge macros
//! Rust Type                   | Slosh Type & Traits   <br>&emsp; <br> S -> R Convert Slosh -> Rust <br> R -> S Convert Rust -> Slosh                                             |
//! ----------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------|
//! [`String`]                  | [`Value`]`::String`         |
//!                             |                             | S -> R
//!                             |                             |     &emsp;- [`SlInto`] [`String`] for `&`[`Value`]
//!                             |                             | R -> S
//!                             |                             |     &emsp;- [`SlFrom`] `&`[`Value`] for [`String`]
//!                             |                             |
//! `&`[`String`]               | [`Value`]`::String`         |
//!                             |                             | S -> R
//!                             |                             |     &emsp;- [`SlInto`] `&`[`String`] for `&`[`Value`]
//!                             |                             | R -> S
//!                             |                             |     &emsp;- take [`String`]
//!                             |                             |     &emsp;* uses Clone unless TODO PC [the extant value problem]
//!                             |                             |
//! `&mut `[`String`]           | [`Value`]`::String`         |
//!                             |                             | S -> R
//!                             |                             |     &emsp;- [`SlAsMut`] [`String`] for `&`[`Value`]
//!                             |                             | R -> S
//!                             |                             |     &emsp;- take `&mut `[`String`]
//!                             |                             |     &emsp;* uses Clone unless TODO PC [the extant value problem]
//!                             |                             |
//! `&`[`str`]                  | [`Value`]`::String` / [`Value`]`::StringConst` |
//!                             |                             | S -> R
//!                             |                             |     &emsp;- [`SlAsRef`] [`str`] for `&`[`Value`]
//!                             |                             | R -> S
//!                             |                             |     &emsp;- [`SlFrom`] for [`Value`]
//!                             |                             |     &emsp;* uses Clone unless TODO PC [the extant value problem]
//!                             |                             |     &emsp;- TODO PC is it even possible to call vm.alloc_string_ro on something that was *newly* created in the current fcn and returned as a RO value OR should that be made as a custom type so the user can declare their intent.
//!                             |                             |
//! [`char`]                    | [`Value`]`::CodePoint`      |
//!                             |                             | S -> R
//!                             |                             |     &emsp;- [`SlInto`] [`char`] for `&`[`Value`]
//!                             |                             | R -> S
//!                             |                             |     &emsp;- [`SlFrom`] `&`[`Value`] for [`char`]
//!                             |                             |
//! [`SloshChar`]               |  [`Value`]`::CharClusterLong` / [`Value`]`::CharCluster` / [`Value`]`::CodePoint` |
//!                             |                             | S -> R
//!                             |                             |     &emsp;- [`SlIntoRef`] [`SloshChar`] for `&`[`Value`]
//!                             |                             | R -> S
//!                             |                             |     &emsp;- [`SlFromRef`] `&`[`Value`] for [`SloshChar`]
//!                             |                             |
//!                             |                             |
//! Value::StringConst          |                             |
//! Value::CharCluster          |                             |
//! Value::CharClusterLong      |                             |
//! Value::Byte                 |                             |
//! Value::Int32                |                             |
//! Value::UInt32               |                             |
//! Value::Int64                |                             |
//! Value::UInt64               |                             |
//! Value::Float64              |                             |
//! Value::Symbol               |                             |
//! Value::Keyword              |                             |
//! Value::Special              |                             |
//! Value::Builtin              |                             |
//! Value::True                 |                             |
//! Value::False                |                             |
//! Value::Nil                  |                             |
//! Value::Undefined            |                             |
//! Value::Vector               |                             |
//! Value::PersistentVec        |                             |
//! Value::VecNode              |                             |
//! Value::PersistentMap        |                             |
//! Value::MapNode              |                             |
//! Value::Map                  |                             |
//! Value::Bytes                |                             |
//! Value::Pair                 |                             |
//! Value::List                 |                             |
//! Value::Lambda               |                             |
//! Value::Closure              |                             |
//! Value::Continuation         |                             |
//! Value::CallFrame            |                             |
//! Value::Value                |                             |
//! Value::Error                |                             |

use std::borrow::Cow;
use bridge_types::SloshChar;
use compile_state::state::SloshVm;
use slvm::{SLOSH_CHAR, Value, VMError, VMResult};

pub trait SlFrom<T>: Sized {
    /// Converts to this type from the input type.
    fn sl_from(value: T, vm: &mut SloshVm) -> VMResult<Self>;
}

pub trait SlInto<T>: Sized {
    /// Converts this type into the (usually inferred) input type.
    fn sl_into(self, vm: &mut SloshVm) -> VMResult<T>;
}

impl<T, U> SlInto<U> for T
where U: SlFrom<T> {
    fn sl_into(self, vm: &mut SloshVm) -> VMResult<U> {
        U::sl_from(self, vm)
    }
}

pub trait SlFromRef<'a, T>: Sized where Self: 'a {
    /// Converts to this type from the input type.
    fn sl_from_ref(value: T, vm: &'a mut SloshVm) -> VMResult<Self>;
}

pub trait SlIntoRef<'a, T>: Sized where T: 'a {
    /// Converts to this type from the input type.
    fn sl_into_ref(self, vm: &'a mut SloshVm) -> VMResult<T>;
}

impl<'a, T, U> SlIntoRef<'a, U> for T
    where U: SlFromRef<'a, T>, U: 'a {
    fn sl_into_ref(self, vm: &'a mut SloshVm) -> VMResult<U> {
        U::sl_from_ref(self, vm)
    }
}

pub trait SlAsRef<'a, T: ?Sized> {

    /// Converts this type into a shared reference of the (usually inferred) input type.
    fn sl_as_ref(&self, vm: &'a mut SloshVm) -> VMResult<&'a T>;
}

// SlAsRef lifts over &
impl<'a, T: ?Sized, U: ?Sized> SlAsRef<'a, U> for &'a T
    where
        T: SlAsRef<'a, U>,
{
    #[inline]
    fn sl_as_ref(&self, vm: &'a mut SloshVm) -> VMResult<&'a U> {
        <T as SlAsRef<'a, U>>::sl_as_ref(*self, vm)
    }
}

// SlAsRef lifts over &mut
impl<'a, T: ?Sized, U: ?Sized> SlAsRef<'a, U> for &'a mut T
    where
        T: SlAsRef<'a, U>,
{
    #[inline]
    fn sl_as_ref(&self, vm: &'a mut SloshVm) -> VMResult<&'a U> {
        <T as SlAsRef<'a, U>>::sl_as_ref(*self, vm)
    }
}

pub trait SlAsMut<'a, T: ?Sized> {
    /// Converts this type into a mutable reference of the (usually inferred) input type.
    fn sl_as_mut(&mut self, vm: &'a mut SloshVm) -> VMResult<&'a mut T>;
}

// SlAsMut lifts over &mut
impl<'a, T: ?Sized, U: ?Sized> SlAsMut<'a, U> for &'a mut T
    where
        T: SlAsMut<'a, U>,
{
    #[inline]
    fn sl_as_mut(&mut self, vm: &'a mut SloshVm) -> VMResult<&'a mut U> {
        (*self).sl_as_mut(vm)
    }
}

// TODO PC work out how the LooseString stuff work as a Cow type
// is what
// #[macro_export]
//macro_rules! try_inner_string {
//    ($fn_name:ident, $expression:expr, $name:ident, $eval:expr) => {{
//        use $crate::ErrorStrings;
//        match &$expression.get().data {
//            Value::String($name, _) => $eval,
//            Value::Symbol($name, _) => $eval,
//            Value::Char($name) => $eval,
//            _ => {
//                return Err($crate::LispError::new(ErrorStrings::mismatched_type(
//                    $fn_name,
//                    &format!(
//                        "{}, {}, or {}, ",
//                        Value::String(Default::default(), Default::default()).to_string(),
//                        Value::Symbol(Default::default(), Default::default()).to_string(),
//                        Value::Char(Default::default()).to_string()
//                    ),
//                    &$expression.to_string(),
//                )))
//            }
//        }
//    }};
//}
//
//impl<'a> SlFrom<&Value> for LooseString<'a, str> {
//    fn sl_from(value: &Value, vm: &'a mut SloshVm) -> VMResult<LooseString<'a, str>> {
//        // TODO PC which other of these types do we consider to be "cast"-able to a
//        // string in the context of Rust functions that implement "this" macro.
//        match value {
//            Value::String(h) => {
//                Ok(LooseString::Borrowed(vm.get_string(*h)))
//            }
//            Value::CodePoint(char) => {
//                Ok(LooseString::Owned(char.to_string()))
//            }
//            Value::CharCluster(l, c) => {
//                let s = format!("{}", String::from_utf8_lossy(&c[0..*l as usize]));
//                Ok(LooseString::Owned(s))
//            }
//            Value::CharClusterLong(h) => {
//                let ch = vm.get_string(*h);
//                Ok(LooseString::Borrowed(ch))
//            }
//            Value::Symbol(i) => {
//                Ok(LooseString::Borrowed(vm.get_interned(*i)))
//            },
//            Value::Keyword(i) => {
//                let s = format!(":{}", vm.get_interned(*i));
//                Ok(LooseString::Owned(s))
//            },
//            Value::StringConst(i) => {
//                let s = format!("\"{}\"", vm.get_interned(*i));
//                Ok(LooseString::Owned(s))
//            },
//            _ => {
//                Err(VMError::new_vm("Wrong type, expected something that can be cast to a string."))
//            }
//        }
//    }
//}

impl SlFrom<&Value> for char {
    fn sl_from(value: &Value, _vm: &mut SloshVm) -> VMResult<Self> {
        match value {
            Value::CodePoint(char) => {
                Ok(*char)
            }
            _ => {
                Err(VMError::new_vm("Wrong type, expected something that can be cast to a char."))
            }
        }
    }
}

impl SlFrom<char> for Value {
    fn sl_from(value: char, _vm: &mut SloshVm) -> VMResult<Self> {
        Ok(Value::CodePoint(value))
    }
}

impl<'a> SlAsRef<'a, str> for &Value {
    fn sl_as_ref(&self, vm: &'a mut SloshVm) -> VMResult<&'a str> {
        match self {
            Value::String(h) => {
                Ok(vm.get_string(*h))
            }
            Value::StringConst(i) => {
                Ok(vm.get_interned(*i))
            }
            _ => {
                Err(VMError::new_vm("Wrong type, expected something that can be cast to a &str."))
            }

        }
    }
}

impl<'a> SlFromRef<'a, &Value> for SloshChar<'a> {
    fn sl_from_ref(value: &Value, vm: &'a mut SloshVm) -> VMResult<Self> {
        match value {
            Value::CodePoint(ch) => {
                Ok(SloshChar::Char(*ch))
            }
            Value::CharCluster(l, c) => {
                Ok(SloshChar::String(Cow::Owned(format!("{}", String::from_utf8_lossy(&c[0..*l as usize])))))
            }
            Value::CharClusterLong(h) => {
                Ok(SloshChar::String(Cow::Borrowed(vm.get_string(*h))))
            }
            _ => {
                Err(VMError::new_vm(format!("Wrong type, expected something that can be cast to a {SLOSH_CHAR}.")))
            }
        }
    }
}

impl<'a> SlFromRef<'a, SloshChar<'a>> for Value {
    fn sl_from_ref(value: SloshChar, vm: &'a mut SloshVm) -> VMResult<Self> {
        match value {
            SloshChar::Char(ch) => {
                Ok(Value::CodePoint(ch))
            }
            SloshChar::String(cow) => {
                match cow {
                    Cow::Borrowed(s) => {
                        Ok(vm.alloc_char(s))
                    }
                    Cow::Owned(s) => {
                        Ok(vm.alloc_char(s.as_str()))
                    }
                }
            }
        }
    }
}

impl<'a> SlAsMut<'a, String> for &Value {
    fn sl_as_mut(&mut self, vm: &'a mut SloshVm) -> VMResult<&'a mut String> {
        match self {
            Value::String(h) => {
                Ok(vm.get_string_mut(*h))
            }
            _ => {
                Err(VMError::new_vm("Wrong type, expected something that can be cast to a &mut String."))
            }
        }
    }
}

impl SlFrom<String> for Value {
    fn sl_from(value: String, vm: &mut SloshVm) -> VMResult<Self> {
        Ok(vm.alloc_string(value))
    }
}

impl<T> SlFrom<&T> for Value where T: ToString + ?Sized {
    fn sl_from(value: &T, vm: &mut SloshVm) -> VMResult<Self> {
        Ok(vm.alloc_string(value.to_string()))
    }
}

impl<T> SlFrom<&mut T> for Value where T: ToString + ?Sized {
    fn sl_from(value: &mut T, vm: &mut SloshVm) -> VMResult<Self> {
        Ok(vm.alloc_string(value.to_string()))
    }
}


// TODO PC preference would be for String to just be Value::String & Value::StringConst
// and let LooseString handle the rest, also avoids needless allocations the user of
// the macro may not care for.
impl SlFrom<&Value> for String {
    fn sl_from(value: &Value, vm: &mut SloshVm) -> VMResult<Self> {
        match value {
            Value::String(h) => {
                Ok(vm.get_string(*h).to_string())
            }
// TODO PC if [`LooseString`] exists then none of these should be implemented
//            Value::CodePoint(char) => {
//                let s = char;
//                Ok(s.encode_utf8(&mut [0; 4]).to_string())
//            }
//            Value::CharCluster(l, c) => {
//                Ok(format!("{}", String::from_utf8_lossy(&c[0..*l as usize])))
//            }
//            Value::CharClusterLong(h) => {
//                Ok(vm.get_string(*h).to_string())
//            }
//            Value::Symbol(i) => {
//                Ok(vm.get_interned(*i).to_string())
//            },
//            Value::Keyword(i) => {
//                Ok(vm.get_interned(*i).to_string())
//            },
//            Value::StringConst(i) => {
//                Ok(vm.get_interned(*i).to_string())
//            },
            _ => {
                Err(VMError::new_vm("Wrong type, expected something that can be cast to a string."))
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use compile_state::state::new_slosh_vm;

    #[test]
    fn try_str_trim() {
        let mut vm = new_slosh_vm();
        let to_trim = " hello world ";
        let val = str_trim_test(&mut vm, to_trim.to_string()).unwrap();
        match val {
            Value::String(handle) => {
                let to_test = vm.get_string(handle);
                assert_eq!(to_test, "hello world");
            }
            _ => {
                panic!("Should return a string!")
            }
        }
    }

    #[test]
    fn try_str_mut() {
        let mut vm = new_slosh_vm();
        let to_mutate = " hello world ";
        let test_str = vm.alloc_string(to_mutate.to_string());
        let args = &[test_str];
        str_test_mut(&mut vm, args).unwrap();
        match args[0] {
            Value::String(handle) => {
                let to_test = vm.get_string(handle);
                assert_eq!(to_test, " hello world 0");
            }
            _ => {
                panic!("Should return a string!")
            }
        }
    }

    fn str_test_mut(vm: &mut SloshVm, args: &[Value]) -> VMResult<()> {
        let fn_name = "str_trim";
        const PARAMS_LEN: usize = 1usize;
        let arg_types: [bridge_types::Param; PARAMS_LEN] =
            [bridge_types::Param {
                handle: bridge_types::TypeHandle::Direct,
                passing_style: bridge_types::PassingStyle::MutReference,
            }];

        let param = arg_types[0usize];
        match param.handle {
            bridge_types::TypeHandle::Direct =>
                match args.get(0usize) {
                    None => {
                        return Err(crate::VMError::new_vm(&*{
                            let res =
                                format!("{} not given enough arguments, expected at least {} arguments, got {}.", fn_name, 1usize, args.len());
                            res
                        }));
                    }
                    Some(mut arg_0) => {
                        {
                            match args.get(PARAMS_LEN) {
                                Some(_) if
                                PARAMS_LEN == 0 ||
                                    arg_types[PARAMS_LEN - 1].handle !=
                                        bridge_types::TypeHandle::VarArgs => {
                                    return Err(crate::VMError::new_vm(&*{
                                        let res =
                                            format!("{} given too many arguments, expected at least {} arguments, got {}.",
                                                    fn_name, 1usize, args.len());
                                        res
                                    }));
                                }
                                _ => {
                                    {
                                        let arg: &mut String = arg_0.sl_as_mut(vm)?;
                                        arg.push_str("0");
                                        Ok(())
                                    }
                                }
                            }
                        }
                    }
                },
            _ => {
                return Err(crate::VMError::new_vm(&*{
                    let res =
                        format!("{} failed to parse its arguments, internal error.",
                                fn_name);
                    res
                }));
            }
        }
    }

    fn str_trim_test(vm: &mut SloshVm, test_str: String) -> VMResult<Value> {
        let test_str = vm.alloc_string(test_str);
        let args = [test_str];
        let fn_name = "str_trim";
        const PARAMS_LEN: usize = 1usize;
        let arg_types: [bridge_types::Param; PARAMS_LEN] =
            [bridge_types::Param {
                handle: bridge_types::TypeHandle::Direct,
                passing_style: bridge_types::PassingStyle::Value,
            }];

        let param = arg_types[0usize];
        match param.handle {
            bridge_types::TypeHandle::Direct =>
                match args.get(0usize) {
                    None => {
                        return Err(crate::VMError::new_vm(&*{
                            let res =
                                format!("{} not given enough arguments, expected at least {} arguments, got {}.", fn_name, 1usize, args.len());
                            res
                        }));
                    }
                    Some(arg_0) => {
                        {
                            match args.get(PARAMS_LEN) {
                                Some(_) if
                                PARAMS_LEN == 0 ||
                                    arg_types[PARAMS_LEN - 1].handle !=
                                        bridge_types::TypeHandle::VarArgs => {
                                    return Err(crate::VMError::new_vm(&*{
                                        let res =
                                            format!("{} given too many arguments, expected at least {} arguments, got {}.",
                                                    fn_name, 1usize, args.len());
                                        res
                                    }));
                                }
                                _ => {
                                    return {
                                        let arg: String = arg_0.sl_into(vm)?;
                                        arg.trim().to_string().sl_into(vm)
                                    }
                                }
                            }
                        }
                    }
                },
            _ => {
                return Err(crate::VMError::new_vm(&*{
                    let res =
                        format!("{} failed to parse its arguments, internal error.",
                                fn_name);
                    res
                }));
            }
        }
    }

    #[test]
    fn test_string_conversions_value_to_rust() {
        let mut vm = new_slosh_vm();
        let vm = &mut vm;
        let test_string = &mut "hello world".to_string();
        let val: Value = test_string.sl_into(vm).expect("&mut String can be converted to Value");
        assert!(matches!(val, Value::String(_)));

        let _s: String = (&val).sl_into(vm).expect("&Value::String can be converted to String");
        let _s: &str = (&val).sl_as_ref(vm).expect("&Value::String can be converted to &str");
        let _s: &mut String = (&val).sl_as_mut(vm).expect("&Value::String can be converted to &mut String");
    }

    #[test]
    fn test_string_conversions_rust_to_value() {
        let mut vm = new_slosh_vm();
        let vm = &mut vm;

        let test_string = "hello world";
        let val: Value = test_string.sl_into(vm).expect("&str can be converted to Value");
        assert!(matches!(val, Value::String(_)));

        let test_string = "hello world".to_string();
        let val: Value = test_string.sl_into(vm).expect("String can be converted to Value");
        assert!(matches!(val, Value::String(_)));

        let test_string = "hello world".to_string();
        let val: Value = (&test_string).sl_into(vm).expect("&String can be converted to Value");
        assert!(matches!(val, Value::String(_)));

        let mut test_string = "hello world".to_string();
        let val: Value = (&mut test_string).sl_into(vm).expect("&String can be converted to Value");
        assert!(matches!(val, Value::String(_)));
    }

    #[test]
    fn test_char_conversions_value_to_rust() {
        let mut vm = new_slosh_vm();
        let vm = &mut vm;

        let test_char = 'न';
        let val = Value::CodePoint(test_char);
        let _c: char = (&val).sl_into(vm).expect("&Value::CodePoint can be converted to char");
    }

    #[test]
    fn test_char_conversions_rust_to_value() {
        let mut vm = new_slosh_vm();
        let vm = &mut vm;

        let test_char: char = 'न';
        let val: Value = test_char.sl_into(vm).expect("char can be converted to Value");
        assert!(matches!(val, Value::CodePoint(_)));
    }

    #[test]
    fn test_char_cluster_conversions_value_to_rust() {
        let mut vm = new_slosh_vm();
        let vm = &mut vm;

        let char_cluster = "ते";
        let val = vm.alloc_char(char_cluster);
        assert!(matches!(val, Value::CharCluster(_, _)));
        let _c: SloshChar = (&val).sl_into_ref(vm).expect("&Value::CharCluster can be converted to SloshChar");

        let char_cluster_long = "👩‍💻";
        let val = vm.alloc_char(char_cluster_long);
        assert!(matches!(val, Value::CharClusterLong(_)));
        let _c: SloshChar = (&val).sl_into_ref(vm).expect("&Value::CharClusterLong can be converted to SloshChar");
    }

    #[test]
    fn test_char_cluster_conversions_rust_to_value() {
        let mut vm = new_slosh_vm();
        let vm = &mut vm;

        let char_cluster = "ते";
        let rust_char_cluster = SloshChar::String(Cow::Owned(char_cluster.to_string()));
        let val: Value = SlFromRef::sl_from_ref(rust_char_cluster, vm).expect("&SloshChar can be converted to &Value");
        assert!(matches!(val, Value::CharCluster(_, _)));

        let char_cluster_long = "👩‍💻";
        let rust_char_cluster = SloshChar::String(Cow::Borrowed(char_cluster_long));
        let val: Value = SlFromRef::sl_from_ref(rust_char_cluster, vm).expect("&SloshChar can be converted to &Value");
        assert!(matches!(val, Value::CharClusterLong(_)));
    }
}