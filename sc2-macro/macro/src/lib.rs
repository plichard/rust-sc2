#[macro_use]
extern crate quote;

use proc_macro::TokenStream;
use regex::Regex;
use syn::{parse_macro_input, Attribute, Data, DeriveInput, ItemEnum, Meta, NestedMeta};

#[proc_macro_derive(FromStr, attributes(enum_from_str))]
pub fn enum_from_str_derive(input: TokenStream) -> TokenStream {
	let item = parse_macro_input!(input as DeriveInput);
	if let Data::Enum(data) = item.data {
		let name = item.ident;
		let variants = data.variants.iter().map(|v| &v.ident);
		// let variants2 = variants.clone().map(|v| format!("{}::{}", name, v));

		let additional_attributes = |a: &Attribute| {
			if a.path.is_ident("enum_from_str") {
				if let Meta::List(list) = a.parse_meta().unwrap() {
					return list.nested.iter().any(|n| {
						if let NestedMeta::Meta(Meta::Path(path)) = n {
							path.is_ident("use_primitives")
						} else {
							false
						}
					});
				} else {
					unreachable!("No options found in attribute `enum_from_str`")
				}
			}
			false
		};
		let other_cases = if item.attrs.iter().any(additional_attributes) {
			quote! {
				n => {
					if let Ok(num) = n.parse() {
						if let Some(result) = Self::from_i64(num) {
							return Ok(result);
						}
					}
					return Err(sc2_macro::ParseEnumError);
				}
			}
		} else {
			quote! {_ => return Err(sc2_macro::ParseEnumError)}
		};
		TokenStream::from(quote! {
			impl std::str::FromStr for #name {
				type Err = sc2_macro::ParseEnumError;

				fn from_str(s: &str) -> Result<Self, Self::Err> {
					Ok(match s {
						#(
							stringify!(#variants) => Self::#variants,
							// #variants2 => Self::#variants,

						)*
						#other_cases,
					})
				}
			}
		})
	} else {
		panic!("Can only derive FromStr for enums")
	}
}

#[proc_macro_attribute]
pub fn variant_checkers(_attr: TokenStream, item: TokenStream) -> TokenStream {
	// let attr = parse_macro_input!(attr as AttributeArgs);
	let item = parse_macro_input!(item as ItemEnum);

	let name = &item.ident;
	let variants = item.variants.iter().map(|v| &v.ident);
	let re = Regex::new(r"[A-Z0-9]{1}[a-z0-9]*").unwrap();
	let snake_variants = variants.clone().map(|v| {
		format_ident!(
			"is_{}",
			re.find_iter(&v.to_string())
				.map(|m| m.as_str().to_ascii_lowercase())
				.collect::<Vec<String>>()
				.join("_")
		)
	});

	TokenStream::from(quote! {
		#item
		impl #name {
			#(
				#[inline]
				pub fn #snake_variants(self) -> bool {
					matches!(self, Self::#variants)
				}
			)*
		}
	})
}
