extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

#[proc_macro]
pub fn not(item: TokenStream) -> TokenStream {
	let item = TokenStream2::from(item);
	quote!(!#item).into()
}
