#[macro_use]
extern crate cpython;
extern  crate bincode;
extern  crate serde;
use serde::Deserialize;
use std::fs;
use std::path::Path;
use cpython::{Python, PyResult};

py_module_initializer!(aom_keyframes, init_aom_keyframes, PyInit_aom_keyframes, |py, m | {
    m.add(py, "__doc__", "Aom keyframes in Rust.. for whatever reason.. rust is good, right?")?;
    m.add(py, "read_struct", py_fn!(py, rust_aom_keyframes(val: &str)))?;
    Ok(())
});

#[derive(Deserialize, Debug)]
struct FrameData {
    frame: i32,
    weight: i32,
    intra_error: i32,
    frame_avg_wavelet_energy: i32,
    coded_error: i32,
    sr_coded_error: i32,
    tr_coded_error: i32,
    pcnt_inter: i32,
    pcnt_motion: i32,
    pcnt_second_ref: i32,
    pcnt_third_ref: i32,
    pcnt_neutral: i32,
    intra_skip_pct: i32,
    inactive_zone_rows: i32,
    inactive_zone_cols: i32,
    mvr: i32,
    mvr_abs: i32,
    mvc: i32,
    mvc_abs: i32,
    mvrv: i32,
    mvcv: i32,
    mv_in_out_count: i32,
    new_mv_count: i32,
    duration: i32,
    count: i32,
    raw_error_stdev: i32,
}




fn rust_aom_keyframes(_py: Python, stat_file: &str) -> PyResult<Vec<i32>>{
    unimplemented!();
}


fn read_struct(stat_file: &str) -> Vec<i32> {

    let stat_path = Path::new(stat_file);



    unimplemented!()


}
