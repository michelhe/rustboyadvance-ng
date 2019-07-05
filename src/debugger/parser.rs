use nom;
use nom::branch::alt;
use nom::bytes::complete::{tag, take_while1, take_while_m_n};
use nom::character::complete::{char, digit1, multispace0, multispace1};
use nom::combinator::{cut, map, map_res, opt};
use nom::error::{context, convert_error, ParseError, VerboseError};
use nom::multi::separated_list;
use nom::sequence::{delimited, preceded, separated_pair, terminated, tuple};
use nom::IResult;

use super::{DebuggerError, DebuggerResult};

#[derive(Debug, PartialEq, Clone)]
pub enum DerefType {
    Word,
    HalfWord,
    Byte,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Value {
    Num(u32),
    Boolean(bool),
    Identifier(String),
    Deref(Box<Value>, DerefType),
}

#[derive(Debug, PartialEq)]
pub enum Expr {
    /// (command-name arg0 arg1 ...)
    Command(Value, Vec<Value>),
    /// constant = value
    Assignment(Value, Value),
    Empty,
}

fn parse_u32_hex<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, u32, E> {
    let (i, _) = context("hex", tag("0x"))(i)?;
    map_res(take_while_m_n(1, 8, |c: char| c.is_digit(16)), |s| {
        u32::from_str_radix(s, 16)
    })(i)
}

fn parse_u32<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, u32, E> {
    context("u32", map_res(digit1, |s| u32::from_str_radix(s, 10)))(i)
}

fn parse_num<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, Value, E> {
    map(alt((parse_u32_hex, parse_u32)), |n| Value::Num(n))(i)
}

fn parse_boolean<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, Value, E> {
    context(
        "bool",
        alt((
            map(tag("true"), |_| Value::Boolean(true)),
            map(tag("false"), |_| Value::Boolean(false)),
        )),
    )(i)
}

fn parse_identifier<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, Value, E> {
    map(
        take_while1(|c: char| c.is_alphanumeric() || c == '_' || c == '-'),
        |s: &str| Value::Identifier(String::from(s)),
    )(i)
}

fn parse_deref_type<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, DerefType, E> {
    delimited(
        char('('),
        alt((
            map(tag("u32*"), |_| DerefType::Word),
            map(tag("u16*"), |_| DerefType::HalfWord),
            map(tag("u8*"), |_| DerefType::Byte),
        )),
        char(')'),
    )(i)
}

fn parse_deref<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, Value, E> {
    context(
        "deref",
        preceded(
            char('*'),
            cut(map(
                tuple((
                    map(opt(parse_deref_type), |t| match t {
                        Some(t) => t,
                        None => DerefType::Word,
                    }),
                    alt((parse_num, parse_identifier)),
                )),
                |(t, v)| Value::Deref(Box::new(v), t),
            )),
        ),
    )(i)
}

fn parse_value<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, Value, E> {
    context(
        "argument",
        alt((parse_boolean, parse_deref, parse_num, parse_identifier)),
    )(i)
}

fn parse_command<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&str, Expr, E> {
    context(
        "command",
        map(
            tuple((
                terminated(parse_identifier, multispace0),
                separated_list(multispace1, parse_value),
            )),
            |(cmd, args)| Expr::Command(cmd, args),
        ),
    )(i)
}

fn parse_assignment<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&str, Expr, E> {
    context(
        "assignment",
        map(
            separated_pair(
                parse_value,
                preceded(multispace0, char('=')),
                cut(preceded(multispace0, parse_value)),
            ),
            |(lvalue, rvalue)| Expr::Assignment(lvalue, rvalue),
        ),
    )(i)
}

fn _parse_expr<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&str, Expr, E> {
    context(
        "expression",
        preceded(
            multispace0,
            alt((
                parse_assignment,
                parse_command,
                map(multispace0, |_| Expr::Empty),
            )),
        ),
    )(i)
}

pub fn parse_expr(i: &str) -> DebuggerResult<Expr> {
    match _parse_expr::<VerboseError<&str>>(i) {
        Ok((_, expr)) => Ok(expr),
        Err(nom::Err::Failure(e)) | Err(nom::Err::Error(e)) => {
            Err(DebuggerError::ParsingError(convert_error(i, e)))
        }
        _ => panic!("unhandled parser error"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_expr() {
        assert_eq!(parse_expr("   "), Ok(Expr::Empty));
    }

    #[test]
    fn test_parse_command_expr() {
        assert_eq!(
            parse_expr("command"),
            Ok(Expr::Command(
                Value::Identifier("command".to_string()),
                vec![]
            ))
        );
        assert_eq!(
            parse_expr("command   arg0   0x1337   true  "),
            Ok(Expr::Command(
                Value::Identifier("command".to_string()),
                vec![
                    Value::Identifier("arg0".to_string()),
                    Value::Num(0x1337),
                    Value::Boolean(true)
                ]
            ))
        );
    }

    #[test]
    fn test_parse_assignment_expr() {
        assert_eq!(
            parse_expr("  pc   = 0x1337 "),
            Ok(Expr::Assignment(
                Value::Identifier("pc".to_string()),
                Value::Num(0x1337)
            ))
        );
        assert_eq!(
            parse_expr("aaa   = false "),
            Ok(Expr::Assignment(
                Value::Identifier("aaa".to_string()),
                Value::Boolean(false)
            ))
        );
        assert_eq!(
            parse_expr("  pc   = lr "),
            Ok(Expr::Assignment(
                Value::Identifier("pc".to_string()),
                Value::Identifier("lr".to_string())
            ))
        );
    }

    #[test]
    fn test_parse_deref() {
        assert_eq!(
            parse_deref::<VerboseError<&str>>("*(u16*)0x1234"),
            Ok((
                "",
                Value::Deref(Box::new(Value::Num(0x1234)), DerefType::HalfWord)
            ))
        );
        assert_eq!(
            parse_deref::<VerboseError<&str>>("*r10"),
            Ok((
                "",
                Value::Deref(
                    Box::new(Value::Identifier("r10".to_string())),
                    DerefType::Word
                )
            ))
        );
    }
}
