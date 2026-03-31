use crate::engine::value::JsValue;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct Environment {
    pub variables: HashMap<String, JsValue>,
    parent: Option<Rc<RefCell<Environment>>>,
}

impl Environment {
    pub fn new(parent: Option<Rc<RefCell<Environment>>>) -> Self {
        Self {
            variables: HashMap::new(),
            parent,
        }
    }

    pub fn define(&mut self, name: String, value: JsValue) {
        self.variables.insert(name, value);
    }

    pub fn define_import(
        &mut self,
        name: String,
        namespace: crate::engine::value::JsObjectMap,
        export_name: String,
    ) {
        self.variables.insert(
            name,
            JsValue::ImportBinding {
                namespace,
                export_name,
            },
        );
    }

    pub fn has_binding(&self, name: &str) -> bool {
        if self.variables.contains_key(name) {
            true
        } else if let Some(parent) = &self.parent {
            parent.borrow().has_binding(name)
        } else {
            false
        }
    }

    pub fn get(&self, name: &str) -> Option<JsValue> {
        if let Some(val) = self.variables.get(name) {
            match val {
                JsValue::ImportBinding {
                    namespace,
                    export_name,
                } => Some(crate::engine::value::resolve_namespace_export(
                    namespace,
                    export_name,
                )),
                _ => Some(val.clone()),
            }
        } else if let Some(parent) = &self.parent {
            parent.borrow().get(name)
        } else {
            None
        }
    }

    pub fn set(&mut self, name: &str, value: JsValue) -> Result<(), String> {
        if let Some(existing) = self.variables.get(name).cloned() {
            match existing {
                JsValue::ImportBinding { .. } => Err(format!(
                    "TypeError: Assignment to imported binding '{}' is not allowed",
                    name
                )),
                _ => {
                    self.variables.insert(name.to_string(), value);
                    Ok(())
                }
            }
        } else if let Some(parent) = &self.parent {
            parent.borrow_mut().set(name, value)
        } else {
            Err(format!("ReferenceError: {} is not defined", name))
        }
    }
}
