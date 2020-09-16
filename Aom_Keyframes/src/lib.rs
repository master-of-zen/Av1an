#[macro_use]
extern crate cpython;

use cpython::{Python, PyResult};
use std::io;
use std::io::Write;
py_module_initializer!(aom_keyframes, init_aom_keyframes, PyInit_aom_keyframes, |py, m | {
    m.add(py, "__doc__", "This module is implemented in Rust")?;
    m.add(py, "count_doubles", py_fn!(py, count_doubles(val: &str)))?;
    Ok(())
});

fn count_doubles(_py: Python, val: &str) -> PyResult<u64> {
    print!("Hello from rust\n");
    print!("Value: {0} \nbee bop rust ", val);
    io::stdout().flush().unwrap();
    Ok(32u64)
}

