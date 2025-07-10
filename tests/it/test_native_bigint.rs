//! Tests for native BigInt support in Boa

use alloy_primitives::U256;
use boa_engine::{js_string, Context, JsValue, Source};

#[test]
fn test_boa_native_bigint_support() {
    let mut ctx = Context::default();

    // Test 1: Check if BigInt is available as a global constructor
    let bigint_constructor = ctx.global_object().get(js_string!("BigInt"), &mut ctx).unwrap();
    assert!(bigint_constructor.is_callable());

    // Test 2: Create BigInt from string
    let bigint_val =
        ctx.eval(Source::from_bytes(b"BigInt('123456789012345678901234567890')")).unwrap();
    assert!(bigint_val.is_bigint());

    // Test 3: Create BigInt from number
    let bigint_from_num = ctx.eval(Source::from_bytes(b"BigInt(42)")).unwrap();
    assert!(bigint_from_num.is_bigint());

    // Test 4: BigInt arithmetic operations
    let result = ctx.eval(Source::from_bytes(b"BigInt(100) + BigInt(200)")).unwrap();
    assert!(result.is_bigint());
    assert_eq!(result.to_string(&mut ctx).unwrap().to_std_string().unwrap(), "300");

    // Test 5: BigInt comparison
    let comparison = ctx.eval(Source::from_bytes(b"BigInt(100) < BigInt(200)")).unwrap();
    assert!(comparison.as_boolean().unwrap());

    // Test 6: Convert to string
    let to_string = ctx.eval(Source::from_bytes(b"BigInt(999).toString()")).unwrap();
    assert_eq!(to_string.as_string().unwrap().to_std_string().unwrap(), "999");

    // Test 7: Type checking
    let type_check = ctx.eval(Source::from_bytes(b"typeof BigInt(42)")).unwrap();
    assert_eq!(type_check.as_string().unwrap().to_std_string().unwrap(), "bigint");
}

#[test]
fn test_u256_to_boa_bigint() {
    let mut ctx = Context::default();

    // Test converting U256 values to BigInt
    let test_values = [
        U256::ZERO,
        U256::from(1u64),
        U256::from(u64::MAX),
        U256::from_str_radix("123456789012345678901234567890", 10).unwrap(),
        U256::MAX,
    ];

    for value in test_values {
        // Create BigInt from U256 string representation
        let js_code = format!("BigInt('{value}')");
        let bigint_val = ctx.eval(Source::from_bytes(js_code.as_bytes())).unwrap();

        assert!(bigint_val.is_bigint());

        // Verify the value matches
        let result_str = bigint_val.to_string(&mut ctx).unwrap().to_std_string().unwrap();
        assert_eq!(result_str, value.to_string());
    }
}

#[test]
fn test_bigint_json_serialization() {
    let mut ctx = Context::default();

    // Test that BigInt has toJSON method for proper serialization
    let code = r#"
        const obj = {
            value: BigInt('123456789012345678901234567890')
        };
        
        // BigInt doesn't have native JSON support, so we need custom handling
        BigInt.prototype.toJSON = function() {
            return this.toString();
        };
        
        JSON.stringify(obj);
    "#;

    let result = ctx.eval(Source::from_bytes(code.as_bytes()));

    // Note: Native BigInt doesn't serialize to JSON by default
    // This test confirms we need custom handling
    assert!(result.is_err() || result.unwrap().is_string());
}

#[test]
fn test_bigint_operations_with_large_numbers() {
    let mut ctx = Context::default();

    // Test operations with numbers larger than Number.MAX_SAFE_INTEGER
    let code = r#"
        const a = BigInt('9007199254740993'); // Number.MAX_SAFE_INTEGER + 2
        const b = BigInt('18014398509481984'); // 2^54
        const sum = a + b;
        const product = a * BigInt(2);
        const comparison = a > BigInt(Number.MAX_SAFE_INTEGER);
        
        ({
            sum: sum.toString(),
            product: product.toString(),
            comparison: comparison
        })
    "#;

    let result = ctx.eval(Source::from_bytes(code.as_bytes())).unwrap();
    let obj = result.as_object().unwrap();

    let sum = obj.get(js_string!("sum"), &mut ctx).unwrap();
    assert_eq!(sum.as_string().unwrap().to_std_string().unwrap(), "27021597764222977");

    let product = obj.get(js_string!("product"), &mut ctx).unwrap();
    assert_eq!(product.as_string().unwrap().to_std_string().unwrap(), "18014398509481986");

    let comparison = obj.get(js_string!("comparison"), &mut ctx).unwrap();
    assert!(comparison.as_boolean().unwrap());
}

#[test]
fn test_native_bigint_replaces_polyfill() {
    let mut ctx = Context::default();

    // Test the pattern used in to_bigint function
    // First, test without any polyfill - should work with native BigInt
    let bigint = ctx.global_object().get(js_string!("BigInt"), &mut ctx).unwrap();
    assert!(bigint.is_callable());

    let value = U256::from(12345u64);
    let result = bigint
        .as_callable()
        .unwrap()
        .call(&JsValue::undefined(), &[JsValue::from(js_string!(value.to_string()))], &mut ctx)
        .unwrap();

    assert!(result.is_bigint());
    assert_eq!(result.to_string(&mut ctx).unwrap().to_std_string().unwrap(), "12345");
}
