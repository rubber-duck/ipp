use super::*;

#[test]
fn borrowed_buffers_reuse_capacity_after_the_owner_is_gone() {
    let original = String::from("publication");
    let mut values = Vec::with_capacity(64);
    values.push(original.as_str());
    let capacity = values.capacity();
    let pointer = values.as_ptr().cast::<()>();
    let empty: Vec<&'static str> = empty_buffer(values);
    drop(original);
    assert!(empty.is_empty());
    assert_eq!(empty.capacity(), capacity);
    assert_eq!(empty.as_ptr().cast::<()>(), pointer);
}
