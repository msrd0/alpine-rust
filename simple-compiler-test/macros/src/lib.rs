extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

#[proc_macro]
pub fn create_false_fn(item: TokenStream) -> TokenStream {
	let item = TokenStream2::from(item);
	quote!(fn #item() -> bool { false }).into()
}
