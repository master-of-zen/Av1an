extern crate cpython;
// Reading
use std::mem::transmute;
use std::fs::read;
use cpython::{Python, PyResult};

/*
py_module_initializer!(aom_keyframes, init_aom_keyframes, PyInit_aom_keyframes, |py, m | {
    m.add(py, "__doc__", "Aom keyframes in Rust.. for whatever reason.. rust is good, right?")?;
    m.add(py, "read_struct", py_fn!(py, rust_aom_keyframes(val: &str)))?;
    Ok(())
});
*/

#[derive(Debug)]
#[repr(C)]
struct FrameData {
    frame: f64,
    weight: f64,
    intra_error: f64,
    frame_avg_wavelet_energy: f64,
    coded_error: f64,
    sr_coded_error: f64,
    tr_coded_error: f64,
    pcnt_inter: f64,
    pcnt_motion: f64,
    pcnt_second_ref: f64,
    pcnt_third_ref: f64,
    pcnt_neutral: f64,
    intra_skip_pct: f64,
    inactive_zone_rows: f64,
    inactive_zone_cols: f64,
    mvr: f64,
    mvr_abs: f64,
    mvc: f64,
    mvc_abs: f64,
    mvrv: f64,
    mvcv: f64,
    mv_in_out_count: f64,
    new_mv_count: f64,
    duration: f64,
    count: f64,
    raw_error_stdev: f64,
}


fn main (){
    read_struct("k");
}


fn rust_aom_keyframes(_py: Python, stat_file: &str) -> PyResult<Vec<i64>>{
    unimplemented!();
}


fn read_struct(stat_file: &str) -> Vec<FrameData> {

    let raw_data: Vec<u8> = read(stat_file).unwrap();
    let frame_list: Vec<FrameData> = unsafe {transmute(raw_data)};
    println!("Frame 1 / {}:\n{:?}", frame_list.len(), frame_list[0]);
    frame_list

}
