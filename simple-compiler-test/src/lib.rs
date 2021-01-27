macros::create_false_fn!(get_false);

#[test]
fn test() {
	assert_eq!(false, get_false());
}
