#[cfg(not(target_arch="wasm32"))]
use std::process;

use readline::mal_readline;

use types::{MalType, MalResult};
use types::MalType::*;
use types::{func_from_lisp, func_for_eval};
use core;
use env::Env;
use reader::read_str;
use printer::{pr_str, println};

// READ
fn read(str: String) -> MalResult {
    read_str(str)
}

fn is_pair(ast: MalType) -> bool {
    match ast {
        MalList(list) | MalVector(list) => list.len() != 0,
        _ => false,
    }
}

fn quasiquote(ast: MalType) -> MalResult {
    if !is_pair(ast.clone()) {
        return Ok(MalList(vec![MalSymbol("quote".to_string()), ast.clone()]));
    }

    let list = seq!(ast);

    let arg1 = match list.get(0) {
        Some(ast) => ast,
        None => return Err("quasiquote: 1 or 2 argument(s) required".to_string()),
    };
    if let &MalSymbol(ref symbol) = arg1 {
        if symbol == "unquote" {
            match list.get(1) {
                Some(ast) => return Ok(ast.clone()),
                None => return Err("unquote: 1 argument required".to_string()),
            };
        }
    }

    if is_pair(arg1.clone()) {
        let arg1_list = seq!(arg1.clone());
        match arg1_list.get(0) {
            Some(arg11) => {
                if let &MalSymbol(ref symbol) = arg11 {
                    if symbol == "splice-unquote" {
                        let arg12 = match arg1_list.get(1) {
                            Some(ast) => ast,
                            None => return Err("splice-unquote: 1 argument required".to_string()),
                        };
                        return Ok(MalList(vec![MalSymbol("concat".to_string()),
                                               arg12.clone(),
                                               try!(quasiquote(MalList((&list[1..]).to_vec())))]));
                    }
                }
            }
            None => {}
        };

    }

    Ok(MalList(vec![MalSymbol("cons".to_string()),
                    try!(quasiquote(arg1.clone())),
                    try!(quasiquote(MalList((&list[1..]).to_vec())))]))
}

fn eval_ast(ast: MalType, env: Env) -> MalResult {
    match ast {
        MalSymbol(ref v) => {
            match env.clone().get(v.to_string()) {
                Some(ast) => Ok(ast.clone()),
                None => return Err(format!("{} not found", v)),
            }
        }
        MalList(list) => {
            let mut new_list = vec![];
            for ast in list {
                new_list.push(try!(eval(ast, env.clone())));
            }
            Ok(MalList(new_list))
        }
        MalVector(list) => {
            let mut new_list = vec![];
            for ast in list {
                new_list.push(try!(eval(ast, env.clone())));
            }
            Ok(MalVector(new_list))
        }
        MalHashMap(list) => {
            if list.len() % 2 != 0 {
                return Err(format!("invalid hash-map: len = {}", list.len()));
            }

            let mut new_list: Vec<MalType> = vec![];
            for i in 0..list.len() {
                if i % 2 == 1 {
                    continue;
                }
                new_list.push(list[i].clone());
                new_list.push(try!(eval(list[i + 1].clone(), env.clone())));
            }

            Ok(MalHashMap(new_list))
        }
        v => Ok(v),
    }
}

// EVAL
fn eval(ast: MalType, env: Env) -> MalResult {
    let mut ast: MalType = ast;
    let mut env: Env = env;

    'tco: loop {
        let list = match ast {
            MalList(list) => list,
            _ => return eval_ast(ast.clone(), env),
        };
        if list.len() == 0 {
            return Ok(MalList(list));
        }

        {
            let a0 = list.get(0).unwrap();
            match a0 {
                &MalSymbol(ref v) if v == "def!" => {
                    let key = &list[1];
                    let key = match key {
                        &MalSymbol(ref v) => v,
                        _ => {
                            return Err(format!("unexpected symbol. expected: symbol, actual: {:?}",
                                               key))
                        }
                    };
                    let value = &list[2];
                    let ret = try!(eval(value.clone(), env.clone()));
                    return Ok(env.set(key.to_string(), ret));
                }
                &MalSymbol(ref v) if v == "let*" => {
                    let let_env = try!(Env::new(Some(env.clone()), vec![], vec![]));
                    let pairs = &list[1];
                    let expr = &list[2];
                    let list = seq!(pairs.clone());
                    for i in 0..list.len() {
                        if i % 2 == 1 {
                            continue;
                        }
                        let key = &list[i];
                        let value = &list[i + 1];
                        let key = match key {
                            &MalSymbol(ref v) => v,
                            _ => {
                                return Err(format!("unexpected symbol. expected: symbol, actual: \
                                                    {:?}",
                                                   key))
                            }
                        };
                        let_env.set(key.to_string(), try!(eval(value.clone(), let_env.clone())));
                    }

                    ast = expr.clone();
                    continue 'tco;
                }
                &MalSymbol(ref v) if v == "quote" => {
                    let arg = list.get(1);
                    let arg = match arg {
                        Some(v) => v,
                        None => return Err("quote argument is required".to_string()),
                    };
                    return Ok(arg.clone());
                }
                &MalSymbol(ref v) if v == "quasiquote" => {
                    let arg = list.get(1);
                    let arg = match arg {
                        Some(v) => v,
                        None => return Err("quasiquote argument is required".to_string()),
                    };
                    ast = try!(quasiquote(arg.clone()));
                    continue 'tco;
                }
                &MalSymbol(ref v) if v == "do" => {
                    let len = list.len();
                    let exprs = &list[1..(len - 1)];
                    try!(eval_ast(MalList(exprs.to_vec()), env.clone()));
                    ast = list[list.len() - 1].clone();
                    continue 'tco;
                }
                &MalSymbol(ref v) if v == "if" => {
                    let cond = list.get(1);
                    let cond = match cond {
                        Some(v) => v,
                        None => return Err("cond expr is required".to_string()),
                    };
                    let then_expr = list.get(2);
                    let then_expr = match then_expr {
                        Some(v) => v,
                        None => return Err("then expr is required".to_string()),
                    };
                    let else_expr = list.get(3);

                    let b = match try!(eval(cond.clone(), env.clone())) {
                        MalBool(false) | MalNil => false,
                        _ => true,
                    };
                    if b {
                        ast = then_expr.clone();
                    } else if let Some(else_expr) = else_expr {
                        ast = else_expr.clone();
                    } else {
                        ast = MalNil;
                    }
                    continue 'tco;
                }
                &MalSymbol(ref v) if v == "fn*" => {
                    let binds = list.get(1);
                    let binds = match binds {
                        Some(v) => v,
                        None => return Err("binds is required".to_string()),
                    };
                    let binds = seq!(binds.clone());

                    let exprs = list.get(2);
                    let exprs = match exprs {
                        Some(v) => v,
                        None => return Err("exprs is required".to_string()),
                    };

                    return func_from_lisp(eval, env, binds, exprs.clone());
                }
                _ => {}
            };
        }

        let ret = try!(eval_ast(MalList(list), env.clone()));
        let list = seq!(ret);
        if list.len() == 0 {
            return Err("unexpected state: len == 0".to_string());
        }

        let f = &list[0];
        let args = (&list[1..]).to_vec();
        let f = match f {
            &MalFunc(ref f) => f,
            _ => return Err(format!("unexpected symbol. expected: function, actual: {:?}", f)),
        };
        if let Some(v) = try!(f.tco_apply(args.clone())) {
            ast = v.0;
            env = v.1;
            continue 'tco;
        }
        return f.apply(args);
    }
}

// PRINT
fn print(exp: MalType) -> Result<String, String> {
    Ok(pr_str(&exp, true))
}

pub fn rep(str: String, env: &Env) -> Result<String, String> {
    let ast = try!(read(str));
    let exp = try!(eval(ast, env.clone()));
    print(exp)
}

pub fn new_repl_env() -> Env {
    let repl_env = Env::new(None, vec![], vec![]).unwrap();

    // core.EXT: defined using Racket
    for (key, value) in core::ns().iter() {
        repl_env.set(key.to_string(), value.clone());
    }
    repl_env.set("*ARGV*".to_string(), MalList(vec![]));
    repl_env.set("eval".to_string(), func_for_eval(eval, repl_env.clone()));

    // core.mal: defined using the language itself
    match rep("(def! not (fn* (a) (if a false true)))".to_string(),
              &repl_env) {
        Err(x) => panic!("{}", x),
        _ => {}
    };
    match rep(r##"(def! load-file (fn* (f) (eval (read-string (str "(do " (slurp f) ")")))))"##
                  .to_string(),
              &repl_env) {
        Err(x) => panic!("{}", x),
        _ => {}
    };

    repl_env
}

#[cfg(not(target_arch="wasm32"))]
fn load_file(source: String, env: &Env) {
    let load = format!(r##"(load-file "{}")"##, source);
    let ret = rep(load, env);
    match ret {
        Ok(_) => process::exit(0),
        Err(str) => {
            println!("{}", str);
            process::exit(1);
        }
    };
}

#[cfg(target_arch="wasm32")]
fn load_file(_source: String, _env: &Env) {
    unimplemented!()
}

pub fn run(args: Vec<String>) {
    let repl_env = new_repl_env();

    if 2 <= args.len() {
        let source = args.get(1).unwrap();

        let args = args.iter().skip(2).map(|str| MalString(str.clone())).collect::<Vec<_>>();
        repl_env.set("*ARGV*".to_string(), MalList(args));

        load_file(source.to_string(), &repl_env);
        return;
    }

    loop {
        let line = mal_readline("user> ");
        if let None = line {
            break;
        }
        let result = rep(line.unwrap(), &repl_env);
        match result {
            Ok(message) => println(message),
            Err(message) => println(message),
        }
    }
}
