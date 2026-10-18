use std::collections::HashMap;

use andean_condor::models::encoder::cli_parameter::CLIParameter;
use anyhow::{Result, bail};
use nom::{
    IResult,
    Parser,
    branch::alt,
    bytes::complete::{tag, take_till1},
    character::complete::{char, multispace0, multispace1},
    combinator::success,
    multi::many0,
    sequence::{preceded, separated_pair},
};
use thiserror::Error;
use tracing::{debug, error};

pub struct EncoderParamsParser;

impl EncoderParamsParser {
    #[inline]
    pub fn parse_string(input: &str) -> Result<HashMap<String, CLIParameter>> {
        let mut map = HashMap::new();
        // many0 with preceded handles whitespace-separated items
        let mut parser = many0(preceded(multispace0, Self::parse_parameter));

        match parser.parse(input) {
            Ok((_, items)) => {
                for (name, param) in items {
                    map.insert(name, param);
                }
                Ok(map)
            },
            Err(nom_error) => {
                debug!("Error parsing parameters: {}", nom_error);
                let error = EncoderParamsParserError::InvalidParams(input.to_owned());
                error!("Invalid encoder parameters: {}", error);
                bail!(error)
            },
        }
    }

    fn parse_parameter(input: &str) -> IResult<&str, (String, CLIParameter)> {
        alt((
            Self::parse_equals_pair, // [-/--][key]=[value]
            Self::parse_space_pair,  // [-/--][key] [value]
            Self::parse_flag,        // [-/--][flag]
        ))
        .parse(input)
    }

    fn parse_equals_pair(input: &str) -> IResult<&str, (String, CLIParameter)> {
        let (input, prefix) = Self::parse_prefix(input)?;
        let (input, (name, value)) = separated_pair(
            take_till1(|c| c == '=' || c == ' '),
            char('='),
            take_till1(|c| c == ' '),
        )
        .parse(input)?;

        Ok((
            input,
            (name.to_string(), Self::to_cli_parameter(prefix, "=", value)),
        ))
    }

    fn parse_space_pair(input: &str) -> IResult<&str, (String, CLIParameter)> {
        let (input, prefix) = Self::parse_prefix(input)?;
        let (input, name) = take_till1(|c| c == ' ').parse(input)?;
        let (input, _) = multispace1(input)?;
        let (input, value) = take_till1(|c| c == ' ').parse(input)?;

        Ok((
            input,
            (name.to_string(), Self::to_cli_parameter(prefix, " ", value)),
        ))
    }

    fn parse_flag(input: &str) -> IResult<&str, (String, CLIParameter)> {
        let (input, prefix) = Self::parse_prefix(input)?;
        // Take until space or end of string
        let (input, name) = take_till1(|c| c == ' ').parse(input)?;

        Ok((
            input,
            (name.to_string(), CLIParameter::Bool {
                prefix: prefix.to_string(),
                value:  true,
            }),
        ))
    }

    /// An optional `-` or `--` before the name. The flag style is not assumed.
    /// A missing prefix stays empty so the parameter is passed through exactly
    /// as written.
    fn parse_prefix(input: &str) -> IResult<&str, &str> {
        alt((tag("--"), tag("-"), success(""))).parse(input)
    }

    fn to_cli_parameter(prefix: &str, delim: &str, val: &str) -> CLIParameter {
        val.parse::<f64>().map_or_else(
            |_| CLIParameter::String {
                prefix:    prefix.to_string(),
                delimiter: delim.to_string(),
                value:     val.to_string(),
            },
            |num| CLIParameter::Number {
                prefix:    prefix.to_string(),
                delimiter: delim.to_string(),
                value:     num,
            },
        )
    }
}

#[derive(Debug, Error)]
pub enum EncoderParamsParserError {
    #[error("Invalid parameter: {0}")]
    InvalidParams(String),
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    #[test]
    fn parse_empty_string() {
        let result = EncoderParamsParser::parse_string("");
        assert!(
            result.is_ok_and(|result| { result.is_empty() }),
            "input contains no parameters"
        );
        // assert!(result.is_empty(), "empty input should produce empty map");
    }

    #[test]
    fn parse_single_dash_flag() {
        let result = EncoderParamsParser::parse_string("--flag");
        assert_matches!(result, Ok(_), "input should parse");
        let result = result.expect("input should parse");
        assert_eq!(result.len(), 1, "input contains 1 parameter");
        assert_matches!(
            result["flag"],
            CLIParameter::Bool {
                ref prefix,
                value,
            } if prefix == "--" && value,
            "input contains true flag preceded by \"--\""
        );
    }

    #[test]
    fn parse_single_short_flag() {
        let result = EncoderParamsParser::parse_string("-f");
        assert!(result.is_ok(), "input should parse");
        let result = result.expect("input should parse");
        assert_eq!(result.len(), 1, "input contains 1 parameter");
        assert_matches!(
            result["f"],
            CLIParameter::Bool {
                ref prefix,
                value,
            } if prefix == "-" && value,
            "input contains true short flag preceded by \"-\""
        );
    }

    #[test]
    fn parse_equals_pair_string() {
        let result = EncoderParamsParser::parse_string("--key=value");
        assert!(result.is_ok(), "input should parse");
        let result = result.expect("input should parse");
        assert_eq!(result.len(), 1, "input contains 1 parameter");
        assert_matches!(result["key"], CLIParameter::String { ref prefix, ref delimiter, ref value } if prefix == "--" && delimiter == "=" && value == "value", "input contains key value preceded by \"--\" separated by \"=\"");
    }

    #[test]
    fn parse_equals_pair_number() {
        let result = EncoderParamsParser::parse_string("--crf=23");
        assert!(result.is_ok(), "input should parse");
        let result = result.expect("input should parse");
        assert_eq!(result.len(), 1, "input contains 1 parameter");
        assert_matches!(result["crf"], CLIParameter::Number { ref prefix, ref delimiter, value } if prefix == "--" && delimiter == "=" && value == 23.0, "input contains crf 23 preceded by \"--\" separated by \"=\"");
    }

    #[test]
    fn parse_space_pair_string() {
        let result = EncoderParamsParser::parse_string("--key value");
        assert!(result.is_ok(), "input should parse");
        let result = result.expect("input should parse");
        assert_eq!(result.len(), 1, "input contains 1 parameter");
        assert_matches!(result["key"], CLIParameter::String { ref prefix, ref delimiter, ref value } if prefix == "--" && delimiter == " " && value == "value", "input contains key value preceded by \"--\" separated by \" \"");
    }

    #[test]
    fn parse_space_pair_number() {
        let result = EncoderParamsParser::parse_string("--crf 30");
        assert!(result.is_ok(), "input should parse");
        let result = result.expect("input should parse");
        assert_eq!(result.len(), 1, "input contains 1 parameter");
        assert_matches!(result["crf"], CLIParameter::Number { ref prefix, ref delimiter, value } if prefix == "--" && delimiter == " " && value == 30.0, "input contains crf 30 preceded by \"--\" separated by \" \"");
    }

    #[test]
    fn parse_dashless_pairs_keep_an_empty_prefix() {
        let result =
            EncoderParamsParser::parse_string("crf=16 tune=0").expect("dash-less pairs parse");
        assert_eq!(result.len(), 2, "input contains 2 parameters");
        assert_matches!(
            result["crf"],
            CLIParameter::Number { ref prefix, ref delimiter, value }
                if prefix.is_empty() && delimiter == "=" && value == 16.0,
            "crf=16 keeps its empty prefix instead of gaining one"
        );
        assert_matches!(
            result["tune"],
            CLIParameter::Number { ref prefix, ref delimiter, value }
                if prefix.is_empty() && delimiter == "=" && value == 0.0,
            "tune=0 keeps its empty prefix instead of gaining one"
        );
    }

    #[test]
    fn parse_dashless_space_pair() {
        let result = EncoderParamsParser::parse_string("crf 16").expect("parses");
        assert_matches!(
            result["crf"],
            CLIParameter::Number { ref prefix, ref delimiter, value }
                if prefix.is_empty() && delimiter == " " && value == 16.0,
            "a space-separated pair works without a dash too"
        );
    }

    #[test]
    fn parse_multiple_params() {
        let result = EncoderParamsParser::parse_string("--key1 val1 --key2=val2 -f");
        assert!(result.is_ok(), "input should parse");
        let result = result.expect("input should parse");
        assert_eq!(result.len(), 3, "input contains 3 parameters");
        assert!(result.contains_key("key1"), "input contains key1");
        assert!(result.contains_key("key2"), "input contains key2");
        assert!(result.contains_key("f"), "input contains f");
    }

    // TODO: Restrict allowed characters in parameter names so the "," isn't
    // allowed (current parses as a flag) #[test]
    // fn parse_unsupported_delimiters() {
    //     let result = EncoderParamsParser::parse_string("--invalid-param,43");
    //     assert_matches!(result, Err(_), "input should not parse");
    // }
}
