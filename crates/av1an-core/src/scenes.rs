use std::{
    collections::HashMap,
    process::{exit, Command},
    str::FromStr,
};

use anyhow::{anyhow, bail, Result};
use itertools::Itertools;
use nom::{
    branch::alt,
    bytes::complete::{tag, take_till, take_while},
    character::complete::{char, digit1, space1},
    combinator::{map, map_res, opt, recognize, rest},
    multi::{many1, separated_list0},
    sequence::{preceded, tuple},
};
use serde::{Deserialize, Serialize};

use crate::{
    context::Av1anContext,
    parse::valid_params,
    settings::{invalid_params, suggest_fix},
    Encoder,
};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Scene {
    pub start_frame: usize,
    // Reminding again that end_frame is *exclusive*
    pub end_frame:   usize,
}
