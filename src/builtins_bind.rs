use std::collections::HashMap;
use std::hash::BuildHasher;

use crate::builtins_util::*;
use crate::environment::*;
use crate::eval::*;
use crate::interner::*;
use crate::types::*;

fn builtin_lex(
    environment: &mut Environment,
    args: &mut dyn Iterator<Item = Expression>,
) -> Result<Expression, LispError> {
    let outer = Some(get_current_scope(environment));
    // Make sure not to return without popping this off the scope stack.
    environment.scopes.push(build_new_scope(outer));
    let mut ret: Option<Expression> = None;
    for arg in args {
        if let Some(ret) = ret {
            ret.resolve(environment).map_err(|e| {
                environment.scopes.pop();
                e
            })?;
        }
        ret = Some(eval_nr(environment, arg.clone_root()).map_err(|e| {
            environment.scopes.pop();
            e
        })?);
    }
    environment.scopes.pop();
    Ok(ret.unwrap_or_else(Expression::make_nil))
}

fn proc_set_vars<'a>(
    environment: &mut Environment,
    args: &'a mut dyn Iterator<Item = Expression>,
) -> Result<(&'static str, Option<String>, Expression), LispError> {
    if let Some(key) = args.next() {
        if let Some(arg1) = args.next() {
            let key = match key.get().data {
                ExpEnum::Atom(Atom::Symbol(s)) => s,
                _ => {
                    return Err(LispError::new("first form (binding key) must be a symbol"));
                }
            };
            if let Some(arg2) = args.next() {
                if args.next().is_none() {
                    let doc_str = if let Ok(docs) = eval(environment, arg1)?.as_string(environment)
                    {
                        Some(docs)
                    } else {
                        None
                    };
                    return Ok((key, doc_str, arg2));
                }
            } else {
                return Ok((key, None, arg1));
            }
        }
    }
    Err(LispError::new(
        "bindings requires a key, optional docstring and value",
    ))
}

fn val_to_reference(
    environment: &mut Environment,
    namespace: Option<&'static str>,
    doc_string: Option<String>,
    val_in: Expression,
) -> Result<(Reference, Expression), LispError> {
    let val_in_d = val_in.get();
    if let ExpEnum::Atom(Atom::Symbol(s)) = &val_in_d.data {
        if let Some(reference) = get_expression(environment, s) {
            drop(val_in_d); // Free read lock on val_in.
            Ok((reference, eval(environment, val_in)?))
        } else {
            drop(val_in_d); // Free read lock on val_in.
            let val = eval(environment, val_in)?;
            Ok((
                Reference::new_rooted(
                    val.clone_root(),
                    RefMetaData {
                        namespace,
                        doc_string,
                    },
                ),
                val,
            ))
        }
    } else {
        drop(val_in_d); // Free read lock on val_in.
        let val = eval(environment, val_in)?;
        Ok((
            Reference::new_rooted(
                val.clone_root(),
                RefMetaData {
                    namespace,
                    doc_string,
                },
            ),
            val,
        ))
    }
}

fn builtin_set(
    environment: &mut Environment,
    args: &mut dyn Iterator<Item = Expression>,
) -> Result<Expression, LispError> {
    let (key, doc_str, val) = proc_set_vars(environment, args)?;
    if environment.dynamic_scope.contains_key(key) {
        let (reference, val) = val_to_reference(environment, None, doc_str, val)?;
        environment.dynamic_scope.insert(key, reference);
        Ok(val)
    } else if let Some(scope) = get_symbols_scope(environment, &key) {
        let name = scope.borrow().name;
        let (reference, val) = val_to_reference(environment, name, doc_str, val)?;
        scope.borrow_mut().data.insert(key, reference);
        Ok(val)
    } else {
        Err(LispError::new(
            "set's first form must evaluate to an existing symbol",
        ))
    }
}

fn builtin_def(
    environment: &mut Environment,
    args: &mut dyn Iterator<Item = Expression>,
) -> Result<Expression, LispError> {
    fn current_namespace(environment: &mut Environment) -> Option<&'static str> {
        environment.namespace.borrow().name
    }
    let (key, doc_string, val) = proc_set_vars(environment, args)?;
    if key.contains("::") {
        // namespace reference.
        let mut key_i = key.splitn(2, "::");
        if let Some(namespace) = key_i.next() {
            if let Some(key) = key_i.next() {
                let namespace = if namespace == "ns" {
                    current_namespace(environment)
                        .unwrap_or_else(|| environment.interner.intern("NO_NAME"))
                } else {
                    namespace
                };
                let mut scope = Some(get_current_scope(environment));
                while let Some(in_scope) = scope {
                    let name = in_scope.borrow().name;
                    if let Some(name) = name {
                        if name == namespace {
                            let (reference, val) =
                                val_to_reference(environment, Some(name), doc_string, val)?;
                            in_scope.borrow_mut().data.insert(key, reference);
                            return Ok(val);
                        }
                    }
                    scope = in_scope.borrow().outer.clone();
                }
            }
        }
        let msg = format!(
            "def namespaced symbol {} not valid or namespace not a parent namespace",
            key
        );
        Err(LispError::new(msg))
    } else {
        let ns = current_namespace(environment);
        let (reference, val) = val_to_reference(environment, ns, doc_string, val)?;
        set_expression_current_namespace(environment, key, reference);
        //set_expression_current_ref(environment, key, reference);
        Ok(val)
    }
}

fn builtin_var(
    environment: &mut Environment,
    args: &mut dyn Iterator<Item = Expression>,
) -> Result<Expression, LispError> {
    let (key, doc_string, val) = proc_set_vars(environment, args)?;
    if !environment.scopes.is_empty() {
        if environment
            .scopes
            .last()
            .unwrap()
            .borrow()
            .data
            .contains_key(key)
        {
            Err(LispError::new(format!(
                "var: Symbol {} already exists in local scope, use set! to change it",
                key
            )))
        } else {
            let (reference, val) = val_to_reference(environment, None, doc_string, val)?;
            environment
                .scopes
                .last()
                .unwrap()
                .borrow_mut()
                .data
                .insert(key, reference);
            Ok(val)
        }
    } else {
        Err(LispError::new(
            "var: Can only be used in a local lexical scope (not a namespace- use def for that)",
        ))
    }
}

fn builtin_undef(
    environment: &mut Environment,
    args: &mut dyn Iterator<Item = Expression>,
) -> Result<Expression, LispError> {
    if let Some(key) = args.next() {
        if args.next().is_none() {
            let key_d = &key.get().data;
            if let ExpEnum::Atom(Atom::Symbol(k)) = key_d {
                return if let Some(rexp) = remove_expression_current(environment, &k) {
                    Ok(rexp.exp)
                } else {
                    let msg = format!("undef: symbol {} not defined in current scope (can only undef symbols in current scope).", k);
                    Err(LispError::new(msg))
                };
            }
        }
    }
    Err(LispError::new(
        "undef: can only have one expression (symbol)",
    ))
}

fn builtin_dyn(
    environment: &mut Environment,
    args: &mut dyn Iterator<Item = Expression>,
) -> Result<Expression, LispError> {
    let (key, val) = if let Some(key) = args.next() {
        let key = eval(environment, key)?;
        if let Some(val) = args.next() {
            let key = match key.get().data {
                ExpEnum::Atom(Atom::Symbol(s)) => s,
                _ => {
                    return Err(LispError::new(
                        "first form (binding key) must evaluate to a symbol",
                    ));
                }
            };
            let val = eval(environment, val)?;
            (key, val)
        } else {
            return Err(LispError::new("dyn requires a key and value"));
        }
    } else {
        return Err(LispError::new("dyn requires a key and value"));
    };
    let old_val = if environment.dynamic_scope.contains_key(key) {
        Some(environment.dynamic_scope.remove(key).unwrap())
    } else {
        None
    };
    if let Some(exp) = args.next() {
        if args.next().is_none() {
            environment.dynamic_scope.insert(
                key,
                Reference::new_rooted(
                    val,
                    RefMetaData {
                        namespace: None,
                        doc_string: None,
                    },
                ),
            );
            let res = eval(environment, exp);
            if let Some(old_val) = old_val {
                environment.dynamic_scope.insert(key, old_val);
            } else {
                environment.dynamic_scope.remove(key);
            }
            return res;
        }
    }
    Err(LispError::new(
        "dyn requires three expressions (symbol, value, form to evaluate)",
    ))
}

fn builtin_is_def(
    environment: &mut Environment,
    args: &mut dyn Iterator<Item = Expression>,
) -> Result<Expression, LispError> {
    fn get_ret(environment: &mut Environment, name: &str) -> Result<Expression, LispError> {
        if is_expression(environment, name) {
            Ok(Expression::alloc_data(ExpEnum::Atom(Atom::True)))
        } else {
            Ok(Expression::alloc_data(ExpEnum::Nil))
        }
    }
    fn do_list(environment: &mut Environment, key: Expression) -> Result<Expression, LispError> {
        let new_key = eval(environment, key)?;
        let new_key_d = new_key.get();
        if let ExpEnum::Atom(Atom::Symbol(s)) = &new_key_d.data {
            get_ret(environment, s)
        } else {
            Err(LispError::new("def?: takes a symbol to lookup (list must eval to symbol)"))
        }
    }
    if let Some(key) = args.next() {
        params_done(args, "def?")?;
        match &key.get().data {
            ExpEnum::Atom(Atom::Symbol(s)) => get_ret(environment, s),
            ExpEnum::Pair(_, _) => do_list(environment, key.clone()),
            ExpEnum::Vector(_) => do_list(environment, key.clone()),
            _ => Err(LispError::new("def?: takes a symbol to lookup")),
        }
    } else {
        Err(LispError::new("def? takes one form (symbol)"))
    }
}

fn builtin_ref(
    environment: &mut Environment,
    args: &mut dyn Iterator<Item = Expression>,
) -> Result<Expression, LispError> {
    if let Some(key) = args.next() {
        //let idx = param_eval(environment, args, "values-nth")?;
        //let vals = param_eval(environment, args, "values-nth")?;
        params_done(args, "ref")?;
        match &key.get().data {
            ExpEnum::Atom(Atom::Symbol(s)) => {
                if let Some(form) = get_expression(environment, s) {
                    Ok(form.exp)
                } else {
                    Err(LispError::new(format!("ref: symbol {} not bound", s)))
                }
            }
            _ => Err(LispError::new("ref: takes a bound symbol")),
        }
    } else {
        Err(LispError::new("ref: takes one form (symbol)"))
    }
}

pub fn add_bind_builtins<S: BuildHasher>(
    interner: &mut Interner,
    data: &mut HashMap<&'static str, Reference, S>,
) {
    let root = interner.intern("root");
    data.insert(
        interner.intern("lex"),
        Expression::make_special(
            builtin_lex,
            "Usage: (lex exp0 ... expN) -> expN

Evaluatate each form and return the last like do but it creates a new lexical scope around the call.
This is basically like wrapping in a fn call but without the fn call or like a let
without the initial bindings (you can use var to bind symbols in the new scope instead).

Section: core

Example:
(def test-do-one \"One1\")
(def test-do-two \"Two1\")
(def test-do-three (lex (var test-do-one \"One\")(set! test-do-two \"Two\")(test::assert-equal \"One\" test-do-one)\"Three\"))
(test::assert-equal \"One1\" test-do-one)
(test::assert-equal \"Two\" test-do-two)
(test::assert-equal \"Three\" test-do-three)
",
            root,
        ),
    );
    data.insert(
        interner.intern("set!"),
        Expression::make_special(
            builtin_set,
            "Usage: (set! symbol expression) -> expression

Sets an existing expression in the current scope(s).  Return the expression that was set.
Symbol is not evaluted.

Set will set the first binding it finds starting in the current scope and then
trying enclosing scopes until exhausted.

Section: core

Example:
(def test-do-one nil)
(def test-do-two nil)
(def test-do-three (do (set! test-do-one \"One\")(set! test-do-two \"Two\")\"Three\"))
(test::assert-equal \"One\" test-do-one)
(test::assert-equal \"Two\" test-do-two)
(test::assert-equal \"Three\" test-do-three)
(let ((test-do-one nil))
    ; set the currently scoped value.
    (test::assert-equal \"1111\" (set! test-do-one \"1111\"))
    (test::assert-equal \"1111\" test-do-one))
; Original outer scope not changed.
(test::assert-equal \"One\" test-do-one)
",
            root,
        ),
    );
    data.insert(
        interner.intern("def"),
        Expression::make_special(
            builtin_def,
            "Usage: (def symbol expression) -> expression

Adds an expression to the current namespace.  Return the expression that was defined.
Symbol is not evaluted.

Section: core

Example:
(def test-do-one nil)
(def test-do-two nil)
(def test-do-three (do (set! test-do-one \"One\")(set! test-do-two \"Two\")\"Three\"))
(test::assert-equal \"One\" test-do-one)
(test::assert-equal \"Two\" test-do-two)
(test::assert-equal \"Three\" test-do-three)
(let ((test-do-one nil))
    ; Add this to tthe let's scope (shadow the outer test-do-two).
    (test::assert-equal \"Default\" (def ns::test-do-four \"Default\"))
    ; set the currently scoped value.
    (set! test-do-one \"1111\")
    (set! test-do-two \"2222\")
    (test::assert-equal \"1111\" test-do-one)
    (test::assert-equal \"2222\" test-do-two)
    (test::assert-equal \"Default\" test-do-four))
; Original outer scope not changed.
(test::assert-equal \"One\" test-do-one)
(test::assert-equal \"Default\" test-do-four)
",
            root,
        ),
    );
    data.insert(
        interner.intern("var"),
        Expression::make_special(
            builtin_var,
            "Usage: (var symbol expression) -> expression

Adds an expression to the current lexical scope.  Return the expression that was defined.
This will not add to a namespace (use def for that), use it within functions or
lex forms to create local bindings.
Symbol is not evaluted.

Section: core

Example:
(lex
(var test-do-one nil)
(var test-do-two nil)
(var test-do-three (do (set! test-do-one \"One\")(set! test-do-two \"Two\")\"Three\"))
(test::assert-equal \"One\" test-do-one)
(test::assert-equal \"Two\" test-do-two)
(test::assert-equal \"Three\" test-do-three)
(let ((test-do-one nil))
    ; Add this to tthe let's scope (shadow the outer test-do-two).
    (test::assert-equal \"Default\" (var test-do-two \"Default\"))
    ; set the currently scoped value.
    (set! test-do-one \"1111\")
    (set! test-do-two \"2222\")
    (test::assert-equal \"1111\" test-do-one)
    (test::assert-equal \"2222\" test-do-two))
; Original outer scope not changed.
(test::assert-equal \"One\" test-do-one)
(test::assert-equal \"Two\" test-do-two))
",
            root,
        ),
    );
    data.insert(
        interner.intern("undef"),
        Expression::make_special(
            builtin_undef,
            "Usage: (undef symbol) -> expression

Remove a symbol from the current scope (if it exists).  Returns the expression
that was removed.  It is an error if symbol is not defined in the current scope.
Symbol is not evaluted.

Section: core

Example:
(def test-undef nil)
(test::assert-true (def? test-undef))
(undef test-undef)
(test::assert-false (def? test-undef))
(def test-undef \"undef\")
(test::assert-equal \"undef\" (undef test-undef))
(test::assert-false (def? test-undef))
(test::assert-equal \"undef: symbol test-undef not defined in current scope (can only undef symbols in current scope).\" (cadr (get-error (undef test-undef))))
",
            root,
        ),
    );
    data.insert(
        interner.intern("dyn"),
        Expression::make_function(
            builtin_dyn,
            "Usage: (dyn key value expression) -> result_of_expression

Creates a dynamic binding for key, assigns value to it and evals expression under it.

The binding is gone once the dyn form ends. The binding will take precedence over
any other binding in any scope with that name for any form that evaluates as a
result of the dynamic binding (for instance creating a dynamic binding for
*stdout* will cause all output to stdout to use the new binding in any print's
used indirectly).  Calls to dyn can be nested and previous dynamic values will
be restored as interior dyn's exit.

Section: core

Example:
(defn test-dyn-fn () (print \"Print dyn out\"))
(dyn '*stdout* (open \"/tmp/sl-sh.dyn.test\" :create :truncate) (do (test-dyn-fn)))
(test::assert-equal \"Print dyn out\" (read-line (open \"/tmp/sl-sh.dyn.test\" :read)))
",
            root,
        ),
    );
    data.insert(
        interner.intern("def?"),
        Expression::make_special(
            builtin_is_def,
            "Usage: (def? expression) -> t|nil

Return true if is a defined symbol (bound within the current scope). If expression
is a symbol it is not evaluted and if a list it is evaluted to produce a symbol.

Section: core

Example:
(def test-is-def t)
(def test-is-def2 'test-is-def)
(test::assert-true (def? test-is-def))
(test::assert-true (def? (sym \"test-is-def\")))
(test::assert-true (def? (ref test-is-def2)))
(test::assert-false (def? test-is-def-not-defined))
(test::assert-false (def? (sym \"test-is-def-not-defined\")))
(test::assert-error (def? (ref test-is-def)))
",
            root,
        ),
    );
    data.insert(
        interner.intern("ref"),
        Expression::make_function(
            builtin_ref,
            "Usage: (ref? symbol) -> expression

Return the expression that is referenced by symbol.
Symbol is not evaluated and must be bound in the current scope or an error is raised.

Section: core

Example:
(def test-is-def t)
(test::assert-true (ref test-is-def))
(set! test-is-def '(1 2 3))
(test::assert-equal '(1 2 3) (ref test-is-def))
(test::assert-error (ref test-is-def-no-exist))
",
            root,
        ),
    );
}
