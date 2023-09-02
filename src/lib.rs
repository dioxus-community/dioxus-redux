use std::{rc::Rc, cell::{RefCell, Ref}, ops::{DerefMut, Deref}};

use dioxus_core::Scope;

pub trait Store {
    
}

#[derive(Clone)]
pub struct ReduxStore<S: Store + Clone> {
    inner: Rc<RefCell<S>>
}

pub fn use_init_store<S: Store + Clone + 'static>(cx: Scope, create_store: impl FnOnce() -> S) {
    cx.use_hook(|| {
        cx.provide_context(ReduxStore {
            inner: Rc::new(RefCell::new(create_store()))
        })
    });
}

pub fn use_slice<'a, F: Fn(&S) -> T, S: 'static + 'a + Clone + Store, T: 'a + Clone>(cx: Scope, slicer: F) -> ReduxSlice<T> {
    let store = cx.consume_context::<ReduxStore<S>>().unwrap();
    {
        // TODO: temp
        let store = &store.inner.borrow();
        ReduxSlice::new(slicer(store))
    }
}

pub struct ReduxSlice<S: Clone>(Rc<RefCell<S>>); 

impl<S: Clone> ReduxSlice<S> {
    fn new(slice: S) -> Self {
        Self(Rc::new(RefCell::new(slice)))
    }
}

impl<S: Clone> ReduxSlice<S> {
    pub fn read(&self) -> Ref<S> {
        self.0.borrow()
    }
}

pub mod prelude {
    pub use crate::*;
}