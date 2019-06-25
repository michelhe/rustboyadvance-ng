use std::fmt;

use nom;
use nom::branch::alt;
use nom::bytes::complete::{tag, take_while_m_n};
use nom::character::complete::{alphanumeric0, digit1, multispace0, multispace1};
use nom::combinator::{cut, map, map_res};
use nom::error::{context, convert_error, ParseError, VerboseError};
use nom::multi::separated_list;
use nom::sequence::{preceded, terminated};
use nom::IResult;

use super::{DebuggerError, DebuggerResult};

#[derive(Debug, PartialEq, Clone)]
pub enum Argument {
    Num(u32),
    Boolean(bool),
}

impl fmt::Display for Argument {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Argument::Num(_) => write!(f, "number"),
            Argument::Boolean(_) => write!(f, "boolean"),
        }
    }
}

#[derive(Debug)]
pub struct ParsedLine {
    pub command: String,
    pub args: Vec<Argument>,
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

fn parse_num<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, Argument, E> {
    map(alt((parse_u32_hex, parse_u32)), |n| Argument::Num(n))(i)
}

fn parse_boolean<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, Argument, E> {
    context(
        "bool",
        alt((
            map(tag("true"), |_| Argument::Boolean(true)),
            map(tag("false"), |_| Argument::Boolean(false)),
        )),
    )(i)
}

fn parse_argument<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, Argument, E> {
    context("argument", alt((parse_num, parse_boolean)))(i)
}

fn parse_argument_list<'a, E: ParseError<&'a str>>(
    i: &'a str,
) -> IResult<&'a str, Vec<Argument>, E> {
    context(
        "argument list",
        preceded(multispace0, terminated(separated_list(multispace1, parse_argument), multispace0))
    )(i)
}

fn parse_command_name<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, &'a str, E> {
    context("command", preceded(multispace0, cut(alphanumeric0)))(i)
}

fn parse_line<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&str, ParsedLine, E> {
    let (i, command_name) = parse_command_name(i)?;
    println!("command name = {:?}, remaining {:?}", command_name, i);
    let (i, arguments) = parse_argument_list(i)?;
    println!("command: {} arguments: {:?}", command_name, arguments);

    Ok((
        i,
        ParsedLine {
            command: command_name.to_string(),
            args: arguments,
        },
    ))
}

pub fn parse_command_line(input: &str) -> DebuggerResult<ParsedLine> {
    match parse_line::<VerboseError<&str>>(input) {
        Ok((_, line)) => Ok(line),
        Err(nom::Err::Failure(e)) | Err(nom::Err::Error(e)) => {
            Err(DebuggerError::ParsingError(convert_error(input, e)))
        }
        _ => panic!("unhandled parser error"),
    }
}