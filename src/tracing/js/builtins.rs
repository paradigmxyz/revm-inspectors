//! Builtin functions

use alloy_primitives::{hex, Address, B256, U256};
use boa_engine::{
    builtins::{array_buffer::ArrayBuffer, typed_array::TypedArray},
    js_string,
    object::builtins::{JsArray, JsArrayBuffer, JsTypedArray, JsUint8Array},
    property::Attribute,
    Context, JsArgs, JsError, JsNativeError, JsResult, JsString, JsValue, NativeFunction, Source,
};
use boa_gc::{empty_trace, Finalize, Trace};
use std::{borrow::Borrow, collections::HashSet};

/// bigIntegerJS is the minified version of <https://github.com/peterolson/BigInteger.js>.
pub(crate) const BIG_INT_JS: &str = include_str!("bigint.js");

/// Converts the given `JsValue` to a `serde_json::Value`.
///
/// This first attempts to use the built-in `JSON.stringify` function to convert the value to a JSON
///
/// If that fails it uses boa's to_json function to convert the value to a JSON object
///
/// We use `JSON.stringify` so that `toJSON` properties are used when converting the value to JSON,
/// this ensures the `bigint` is serialized properly.
pub(crate) fn to_serde_value(val: JsValue, ctx: &mut Context) -> JsResult<serde_json::Value> {
    if let Ok(json) = json_stringify(val.clone(), ctx) {
        let json = json.to_std_string().map_err(|err| {
            JsError::from_native(
                JsNativeError::error()
                    .with_message(format!("failed to convert JSON to string: {}", err)),
            )
        })?;
        serde_json::from_str(&json).map_err(|err| {
            JsError::from_native(
                JsNativeError::error().with_message(format!("failed to parse JSON: {}", err)),
            )
        })
    } else {
        val.to_json(ctx)
    }
}

/// Attempts to use the global `JSON` object to stringify the given value.
pub(crate) fn json_stringify(val: JsValue, ctx: &mut Context) -> JsResult<JsString> {
    let json = ctx.global_object().get(js_string!("JSON"), ctx)?;
    let json_obj = json.as_object().ok_or_else(|| {
        JsError::from_native(JsNativeError::typ().with_message("JSON is not an object"))
    })?;

    let stringify = json_obj.get(js_string!("stringify"), ctx)?;

    let stringify = stringify.as_callable().ok_or_else(|| {
        JsError::from_native(JsNativeError::typ().with_message("JSON.stringify is not callable"))
    })?;
    let res = stringify.call(&json, &[val], ctx)?;
    res.to_string(ctx)
}

/// Registers all the builtin functions and global bigint property
///
/// Note: this does not register the `isPrecompiled` builtin, as this requires the precompile
/// addresses, see [PrecompileList::register_callable].
pub(crate) fn register_builtins(ctx: &mut Context) -> JsResult<()> {
    let big_int = ctx.eval(Source::from_bytes(BIG_INT_JS.as_bytes()))?;
    ctx.register_global_property(js_string!("bigint"), big_int, Attribute::all())?;
    ctx.register_global_builtin_callable(
        js_string!("toHex"),
        1,
        NativeFunction::from_fn_ptr(to_hex),
    )?;
    ctx.register_global_callable(js_string!("toWord"), 1, NativeFunction::from_fn_ptr(to_word))?;
    ctx.register_global_callable(
        js_string!("toAddress"),
        1,
        NativeFunction::from_fn_ptr(to_address),
    )?;
    ctx.register_global_callable(
        js_string!("toContract"),
        2,
        NativeFunction::from_fn_ptr(to_contract),
    )?;
    ctx.register_global_callable(
        js_string!("toContract2"),
        3,
        NativeFunction::from_fn_ptr(to_contract2),
    )?;

    Ok(())
}

/// Converts an array, hex string or Uint8Array to a []byte
pub(crate) fn from_buf_value(val: JsValue, context: &mut Context) -> JsResult<Vec<u8>> {
    if let Some(obj) = val.as_object().cloned() {
        if obj.is::<TypedArray>() {
            let array: JsTypedArray = JsTypedArray::from_object(obj)?;
            let len = array.length(context)?;
            let mut buf = Vec::with_capacity(len);
            for i in 0..len {
                let val = array.get(i, context)?;
                buf.push(val.to_number(context)? as u8);
            }
            return Ok(buf);
        } else if obj.is::<ArrayBuffer>() {
            let buf = JsArrayBuffer::from_object(obj)?;
            let buf = buf.data().map(|data| data.to_vec()).ok_or_else(|| {
                JsNativeError::typ().with_message("ArrayBuffer was already detached")
            })?;
            return Ok(buf);
        } else if obj.is::<JsString>() {
            let js_string = obj
                .downcast_ref::<JsString>()
                .ok_or_else(|| JsNativeError::typ().with_message("invalid string type"))?;
            return hex_decode_js_string(js_string.borrow());
        } else if obj.is_array() {
            let array = JsArray::from_object(obj)?;
            let len = array.length(context)?;
            let mut buf = Vec::with_capacity(len as usize);
            for i in 0..len {
                let val = array.get(i, context)?;
                buf.push(val.to_number(context)? as u8);
            }
            return Ok(buf);
        }
    }

    if let Some(js_string) = val.as_string() {
        return hex_decode_js_string(js_string);
    }

    Err(JsError::from_native(
        JsNativeError::typ().with_message(format!("invalid buffer type: {}", val.type_of())),
    ))
}

/// Create a new [JsUint8Array] array buffer from the address' bytes.
pub(crate) fn address_to_byte_array(
    addr: Address,
    context: &mut Context,
) -> JsResult<JsUint8Array> {
    JsUint8Array::from_iter(addr.0, context)
}

/// Create a new [JsUint8Array] array buffer from the address' bytes.
pub(crate) fn address_to_byte_array_value(
    addr: Address,
    context: &mut Context,
) -> JsResult<JsValue> {
    Ok(JsUint8Array::from_iter(addr.0, context)?.into())
}

/// Create a new [JsUint8Array] from byte block.
pub(crate) fn to_byte_array<I>(bytes: I, context: &mut Context) -> JsResult<JsUint8Array>
where
    I: IntoIterator<Item = u8>,
{
    JsUint8Array::from_iter(bytes, context)
}

/// Create a new [JsUint8Array] object from byte block.
pub(crate) fn to_byte_array_value<I>(bytes: I, context: &mut Context) -> JsResult<JsValue>
where
    I: IntoIterator<Item = u8>,
{
    Ok(to_byte_array(bytes, context)?.into())
}

/// Converts a buffer type to an address.
///
/// If the buffer is larger than the address size, it will be cropped from the left
pub(crate) fn bytes_to_address(buf: Vec<u8>) -> Address {
    let mut address = Address::default();
    let mut buf = &buf[..];
    let address_len = address.0.len();
    if buf.len() > address_len {
        // crop from left
        buf = &buf[buf.len() - address.0.len()..];
    }
    let address_slice = &mut address.0[address_len - buf.len()..];
    address_slice.copy_from_slice(buf);
    address
}

/// Converts a buffer type to a hash.
///
/// If the buffer is larger than the hash size, it will be cropped from the left
pub(crate) fn bytes_to_hash(buf: Vec<u8>) -> B256 {
    let mut hash = B256::default();
    let mut buf = &buf[..];
    let hash_len = hash.0.len();
    if buf.len() > hash_len {
        // crop from left
        buf = &buf[buf.len() - hash.0.len()..];
    }
    let hash_slice = &mut hash.0[hash_len - buf.len()..];
    hash_slice.copy_from_slice(buf);
    hash
}

/// Converts a U256 to a bigint using the global bigint property
pub(crate) fn to_bigint(value: U256, ctx: &mut Context) -> JsResult<JsValue> {
    let bigint = ctx.global_object().get(js_string!("bigint"), ctx)?;
    if !bigint.is_callable() {
        return Ok(JsValue::undefined());
    }
    bigint.as_callable().unwrap().call(
        &JsValue::undefined(),
        &[JsValue::from(js_string!(value.to_string()))],
        ctx,
    )
}
/// Takes three arguments: a JavaScript value that represents the sender's address, a string salt
/// value, and the initcode for the contract. Compute the address of a contract created by the
/// sender with the given salt and code hash, then converts the resulting address back into a byte
/// buffer for output.
pub(crate) fn to_contract2(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    // Extract the sender's address, salt and initcode from the arguments
    let from = args.get_or_undefined(0).clone();
    let salt = match args.get_or_undefined(1).to_string(ctx) {
        Ok(js_string) => {
            let buf = hex_decode_js_string(&js_string)?;
            bytes_to_hash(buf)
        }
        Err(_) => {
            return Err(JsError::from_native(
                JsNativeError::typ().with_message("invalid salt type"),
            ))
        }
    };

    let initcode = args.get_or_undefined(2).clone();

    // Convert the sender's address to a byte buffer and then to an Address
    let buf = from_buf_value(from, ctx)?;
    let addr = bytes_to_address(buf);

    // Convert the initcode to a byte buffer
    let code_buf = from_buf_value(initcode, ctx)?;

    // Compute the contract address
    let contract_addr = addr.create2_from_code(salt, code_buf);

    // Convert the contract address to a byte buffer and return it as an ArrayBuffer
    address_to_byte_array_value(contract_addr, ctx)
}

///  Converts the sender's address to a byte buffer
pub(crate) fn to_contract(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    // Extract the sender's address and nonce from the arguments
    let from = args.get_or_undefined(0).clone();
    let nonce = args.get_or_undefined(1).to_number(ctx)? as u64;

    // Convert the sender's address to a byte buffer and then to an Address
    let buf = from_buf_value(from, ctx)?;
    let addr = bytes_to_address(buf);

    // Compute the contract address
    let contract_addr = addr.create(nonce);

    // Convert the contract address to a byte buffer and return it as an ArrayBuffer
    address_to_byte_array_value(contract_addr, ctx)
}

/// Converts a buffer type to an address
pub(crate) fn to_address(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let val = args.get_or_undefined(0).clone();
    let buf = from_buf_value(val, ctx)?;
    let address = bytes_to_address(buf);
    address_to_byte_array_value(address, ctx)
}

/// Converts a buffer type to a word
pub(crate) fn to_word(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let val = args.get_or_undefined(0).clone();
    let buf = from_buf_value(val, ctx)?;
    let hash = bytes_to_hash(buf);
    to_byte_array_value(hash.0, ctx)
}

/// Converts a buffer type to a hex string
pub(crate) fn to_hex(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let val = args.get_or_undefined(0).clone();
    let buf = from_buf_value(val, ctx)?;
    let s = js_string!(hex::encode(buf));
    Ok(JsValue::from(s))
}

/// Decodes a hex decoded js-string
fn hex_decode_js_string(js_string: &JsString) -> JsResult<Vec<u8>> {
    match js_string.to_std_string() {
        Ok(s) => match hex::decode(s.as_str()) {
            Ok(data) => Ok(data),
            Err(err) => Err(JsError::from_native(
                JsNativeError::error().with_message(format!("invalid hex string {s}: {err}",)),
            )),
        },
        Err(err) => Err(JsError::from_native(
            JsNativeError::error()
                .with_message(format!("invalid utf8 string {js_string:?}: {err}",)),
        )),
    }
}

/// A container for all precompile addresses used for the `isPrecompiled` global callable.
#[derive(Clone, Debug)]
pub(crate) struct PrecompileList(pub(crate) HashSet<Address>);

impl PrecompileList {
    /// Registers the global callable `isPrecompiled`
    pub(crate) fn register_callable(self, ctx: &mut Context) -> JsResult<()> {
        let is_precompiled = NativeFunction::from_copy_closure_with_captures(
            move |_this, args, precompiles, ctx| {
                let val = args.get_or_undefined(0).clone();
                let buf = from_buf_value(val, ctx)?;
                let addr = bytes_to_address(buf);
                Ok(precompiles.0.contains(&addr).into())
            },
            self,
        );

        ctx.register_global_callable(js_string!("isPrecompiled"), 1, is_precompiled)?;

        Ok(())
    }
}

impl Finalize for PrecompileList {}

unsafe impl Trace for PrecompileList {
    empty_trace!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_install_bigint() {
        let mut ctx = Context::default();
        let big_int = ctx.eval(Source::from_bytes(BIG_INT_JS.as_bytes())).unwrap();
        let value = JsValue::from(100);
        let result =
            big_int.as_callable().unwrap().call(&JsValue::undefined(), &[value], &mut ctx).unwrap();
        assert_eq!(result.to_string(&mut ctx).unwrap().to_std_string().unwrap(), "100");
    }
}
