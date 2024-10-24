#![allow(unused)]
extern crate proc_macro;
extern crate proc_macro2;
extern crate quote;
extern crate syn;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;

const ABIS: [&str; 31] = [
    "Rust", "C", "C-unwind", "C-cmse-nonsecure-call", "C-cmse-nonsecure-entry", "cdecl", "rust-call",
    "stdcall", "stdcall-unwind", "fastcall", "vectorcall", "thiscall", "thiscall-unwind", "aapcs",
    "win64", "sysv64", "ptx-kernel", "msp430-interrupt", "x86-interrupt", "efiapi", "avr-interrupt",
    "avr-non-blocking-interrupt", "riscv-interrupt-m", "riscv-interrupt-s", "wasm", "system",
    "system-unwind", "rust-intrinsic", "platform-intrinsic", "unadjusted", "none"
];

fn valid_abi(abi: &str) -> bool {                               
    ABIS.contains(&abi)
}

fn validate_abi(abi: syn::LitStr) -> Result<syn::LitStr, syn::Error> {
    let raw = abi.value();
    if valid_abi(&raw) {
        Ok(abi)
    } else {
        let span = abi.span();
        Err(syn::Error::new(span, format!("Invalid ABI '{}', expecting one of {:?}", raw, ABIS)))
    }
}

fn opt_lit_as_opt_val(opt: Option<&syn::LitStr>) -> Option<String> {
    let val = opt?;
    Some(val.value())
}

enum Mode {
    Export,
    Import,
}

/// Macro attribute `reprfn`:
///
/// This macro transforms a function into an ABI-compliant external function or an imported one.
///
/// # Attributes:
/// * `abi`: Optional. Defines the ABI of the function. If omitted, the default ABI is used.
///          If set to `none`, no specific ABI is enforced. Supported ABIs include "C", "Rust", "stdcall", etc.
/// * `name`: Optional. Sets the exported name of the function in C-like linkers. Defaults to the Rust function name.
/// * `mode`: Optional. If set to `export`, it marks the function for external export. If set to `import`,
///           it marks the function as externally imported. If omitted, the macro will automatically infer the mode
///           based on the presence or absence of a function body (presence implies export, absence implies import).
///
/// # Example:
///
/// ```
/// #[reprfn(abi = "C", name = "my_c_function")]
/// pub fn my_function() {
///     // Function body
/// }
/// ```
///
/// will be expanded to
///
/// ```
/// #[no_mangle]
/// #[export_name = "my_c_function"]
/// pub extern "C" fn my_function() {
///     // Function body
/// }
/// ```
#[proc_macro_attribute]
pub fn reprfn(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut abi = None::<syn::LitStr>;
    let mut name = None::<syn::LitStr>;
    let mut feature = None::<syn::LitStr>;
    let mut no_mangle = true;
    let mut support_generics = false;
    let mut mode = None::<Mode>;

    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("abi") {
            let value = validate_abi(meta.value()?.parse()?)?;
            abi = if value.value() == "none" {
                None
            } else {
                Some(value)
            };
            no_mangle = {
                let check = opt_lit_as_opt_val(abi.as_ref());
                match check {
                    Some(val) if val == "Rust" => false,
                    Some(val) if val == "rust-call" => false,
                    Some(val) if val == "rust-intrinsic" => false,
                    _ => true,
                }
            };
            support_generics = {
                let check = opt_lit_as_opt_val(abi.as_ref());
                match check {
                    Some(val) if val == "Rust" => true,
                    Some(val) if val == "rust-call" => true,
                    Some(val) if val == "rust-intrinsic" => true,
                    _ => false,
                }
            };
        } else if meta.path.is_ident("name") {
            let value: syn::LitStr = meta.value()?.parse()?;
            name = if value.value() == "none" {
                None
            } else {
                Some(value)
            };
        } else if meta.path.is_ident("mode") {
            let value: syn::LitStr = meta.value()?.parse()?;
            mode = if value.value() == "none" {
                None
            } else if value.value() == "import" {
                Some(Mode::Import)
            } else if value.value() == "export" {
                Some(Mode::Export)
            } else {
                return Err(meta.error(format!("invalid mode '{}', expecting one of '['none', 'import', 'export']'", value.value())));
            }
        } else if meta.path.is_ident("feature") {
            let value: syn::LitStr = meta.value()?.parse()?;
            feature = if value.value() == "none" {
                None
            } else {
                Some(value)
            };
        }
        Ok(())
    });

    syn::parse_macro_input!(attr with parser);
    let input = syn::parse_macro_input!(item as syn::ItemFn);

    let abi_quote = if let Some(abi_value) = abi {
        quote::quote! {
            extern #abi_value
        }
    } else {
        quote::quote! {
            extern "Rust"
        }
    };

    let name_quote = if let Some(name_value) = name {
        quote::quote! {
            #[export_name = #name_value]
        }
    } else {
        quote::quote! {}
    };

    let no_mangle_quote = if no_mangle {
        quote::quote! {
            #[no_mangle]
        }
    } else {
        quote::quote! {}
    };

    let feature_quote = if let Some(feature_value) = feature {
        quote::quote! {
            #[cfg(feature = #feature_value)]
        }
    } else {
        quote::quote! {}
    };

    let syn::ItemFn { attrs, vis, sig, block } = input;
    let syn::Signature { constness, unsafety, fn_token, ident, inputs, variadic, output, generics, .. } = sig;
    let syn::Generics { lt_token, params, gt_token, where_clause } = generics;

    // Determine mode if not provided, based on the presence of a block or a semicolon
    let inferred_mode = if let Some(_) = mode {
        mode.unwrap()
    } else if block.stmts.is_empty() {
        Mode::Import
    } else {
        Mode::Export
    };

    let expanded = match inferred_mode {
        Mode::Export => {
            if support_generics {
                quote::quote! {
                    #(#attrs)*
                    #feature_quote
                    #name_quote
                    #no_mangle_quote
                    #vis #constness #unsafety #abi_quote #fn_token #ident #lt_token #params #gt_token(#inputs #variadic) #output #where_clause #block
                }
            } else {
                quote::quote! {
                    #(#attrs)*
                    #feature_quote
                    #name_quote
                    #no_mangle_quote
                    #vis #constness #unsafety #abi_quote #fn_token #ident(#inputs #variadic) #output #block
                }
            }
        },
        Mode::Import => {
            if support_generics {
                quote::quote! {
                    #abi_quote {
                        #(#attrs)*
                        #feature_quote
                        #name_quote
                        #vis #fn_token #ident #lt_token #params #gt_token(#inputs #variadic) #output #where_clause #block
                    }
                }
            } else {
                quote::quote! {
                    #abi_quote {
                        #(#attrs)*
                        #feature_quote
                        #name_quote
                        #vis #fn_token #ident(#inputs #variadic) #output #block
                    }
                }
            }
        },
    };

    TokenStream::from(expanded)
}
