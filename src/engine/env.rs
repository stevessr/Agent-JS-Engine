use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use crate::engine::value::JsValue;

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

    pub fn get(&self, name: &str) -> Option<JsValue> {
        if let Some(val) = self.variables.get(name) {
            Some(val.clone())
        } else if let Some(parent) = &self.parent {
            parent.borrow().get(name)
        } else {
            None
        }
    }

    pub fn set(&mut self, name: &str, value: JsValue) -> Result<(), String> {
        if self.variables.contains_key(name) {
            self.variables.insert(name.to_string(), value);
            Ok(())
        } else if let Some(parent) = &self.parent {
            parent.borrow_mut().set(name, value)
        } else {
            Err(format!("ReferenceError: {} is not defined", name))
        }
    }
}
