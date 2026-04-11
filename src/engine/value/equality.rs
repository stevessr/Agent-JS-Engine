impl PartialEq for JsValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (JsValue::Undefined, JsValue::Undefined) => true,
            (JsValue::Null, JsValue::Null) => true,
            (JsValue::Boolean(l), JsValue::Boolean(r)) => l == r,
            (JsValue::Number(l), JsValue::Number(r)) => l == r,
            (JsValue::BigInt(l), JsValue::BigInt(r)) => l == r,
            (JsValue::String(l), JsValue::String(r)) => l == r,
            (JsValue::Array(l), JsValue::Array(r)) => Rc::ptr_eq(l, r),
            (JsValue::Object(l), JsValue::Object(r)) => Rc::ptr_eq(l, r),
            (JsValue::EnvironmentObject(l), JsValue::EnvironmentObject(r)) => Rc::ptr_eq(l, r),
            (JsValue::Promise(l), JsValue::Promise(r)) => Rc::ptr_eq(l, r),
            (JsValue::GeneratorState(l), JsValue::GeneratorState(r)) => Rc::ptr_eq(l, r),
            (
                JsValue::ImportBinding {
                    namespace: l_namespace,
                    export_name: l_export_name,
                },
                JsValue::ImportBinding {
                    namespace: r_namespace,
                    export_name: r_export_name,
                },
            ) => Rc::ptr_eq(l_namespace, r_namespace) && l_export_name == r_export_name,
            (JsValue::Function(l), JsValue::Function(r)) => Rc::ptr_eq(l, r),
            (JsValue::NativeFunction(l), JsValue::NativeFunction(r)) => Rc::ptr_eq(l, r),
            (JsValue::BuiltinFunction(l), JsValue::BuiltinFunction(r)) => Rc::ptr_eq(l, r),
            _ => false,
        }
    }
}
