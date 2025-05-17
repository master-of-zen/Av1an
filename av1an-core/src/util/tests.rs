use std::borrow::Cow;

#[test]
fn count_macro() {
    assert_eq!(crate::count!["rav1e", "-s", "10",], 3);
    assert_eq!(crate::count!["rav1e", "-s", "10"], 3);
    assert_eq!(crate::count!["rav1e" "-s" "10"], 3);
    assert_eq!(crate::count!["rav1e", "-s", "10", ""], 4);
    assert_eq!(crate::count!["rav1e" "-s" "10" ""], 4);
    assert_eq!(crate::count!["rav1e" "-s", "10" ""], 4);
    assert_eq!(crate::count!["rav1e" "-s" "10" "",], 4);
    assert_eq!(crate::count!["rav1e", "-s", "10" "",], 4);
}

#[test]
fn inplace_vec_capacity() {
    let v: Vec<Cow<'static, str>> = crate::inplace_vec!["hello", format!("{}", 4), "world"];
    assert_eq!(v.capacity(), 3);

    // with trailing comma
    let v: Vec<Cow<'static, str>> = crate::inplace_vec!["hello", format!("{}", 4), "world",];
    assert_eq!(v.capacity(), 3);

    let v: Vec<Cow<'static, str>> =
        crate::inplace_vec!["hello", format!("{}", 4), "world", 5.to_string(), "!!!"];
    assert_eq!(v.capacity(), 5);
}

#[test]
fn inplace_vec_is_sound() {
    let v1: Vec<Cow<'static, str>> = crate::inplace_vec!["hello", format!("{}", 4), "world"];
    let v2: Vec<Cow<'static, str>> = crate::into_vec!["hello", format!("{}", 4), "world"];

    assert_eq!(v1, v2);
}
