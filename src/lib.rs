use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet},
    marker::PhantomData,
    rc::Rc,
    sync::Arc,
};

use dioxus_core::{Scope, ScopeId};
use dioxus_hooks::{to_owned, RefCell};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

pub trait Store: Clone {
    type Event;

    fn handle(&mut self, event: Self::Event);
}

#[derive(Clone)]
struct Subscribe {
    cache: CacheEntry,
    unsubscribe: Rc<dyn Fn()>,
}

impl Drop for Subscribe {
    fn drop(&mut self) {
        (self.unsubscribe)();
    }
}

#[derive(Clone)]
struct CacheEntry {
    scopes: Rc<RefCell<HashSet<ScopeId>>>,
    value: Rc<RefCell<Box<dyn Any>>>,
    compare: Rc<RefCell<dyn Fn(&Rc<RefCell<Box<dyn Any>>>) -> bool>>,
}

pub struct ReduxStore<S: Store> {
    inner: Rc<RefCell<S>>,
    event_dispatcher: UnboundedSender<S::Event>,
    cache_map: Rc<RefCell<HashMap<TypeId, CacheEntry>>>,
    schedule_update_any: Arc<dyn Fn(ScopeId)>,
}

impl<S: Store> ReduxStore<S> {
    pub fn handle(&self, event: S::Event) {
        self.inner.borrow_mut().handle(event);

        for (_function, cache) in self.cache_map.borrow().iter() {
            let value = &cache.value;
            let compare = cache.compare.borrow();
            let is_same = compare(value);
            if !is_same {
                for scope_id in cache.scopes.borrow().iter() {
                    (self.schedule_update_any)(*scope_id)
                }
            }
        }
    }

    fn subscribe<V: 'static>(
        &self,
        scope_id: ScopeId,
        function_id: TypeId,
        value: impl FnOnce() -> V,
        compare: impl FnOnce() -> Rc<RefCell<dyn Fn(&Rc<RefCell<Box<dyn Any>>>) -> bool>>,
    ) -> Subscribe {
        let cache = {
            let mut cache_map = self.cache_map.borrow_mut();
            cache_map
                .entry(function_id)
                .and_modify(|entry| {
                    entry.scopes.borrow_mut().insert(scope_id);
                })
                .or_insert_with(|| CacheEntry {
                    scopes: Rc::new(RefCell::new(HashSet::from([scope_id]))),
                    value: Rc::new(RefCell::new(Box::new(value()))),
                    compare: compare(),
                })
                .clone()
        };

        Subscribe {
            cache,
            unsubscribe: {
                let cache_map = self.cache_map.clone();
                Rc::new(move || {
                    let mut cache_map = cache_map.borrow_mut();
                    let is_empty = {
                        let function = cache_map.get_mut(&function_id);
                        if let Some(function) = function {
                            function.scopes.borrow_mut().remove(&scope_id);
                            function.scopes.borrow().is_empty()
                        } else {
                            false
                        }
                    };
                    if is_empty {
                        cache_map.remove(&function_id);
                    }
                })
            },
        }
    }
}

impl<S: Store> Clone for ReduxStore<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            event_dispatcher: self.event_dispatcher.clone(),
            cache_map: self.cache_map.clone(),
            schedule_update_any: self.schedule_update_any.clone(),
        }
    }
}

pub fn use_init_store<S: Store + 'static>(cx: Scope, create_store: impl FnOnce() -> S) {
    cx.use_hook(|| {
        let (event_tx, mut event_rx) = unbounded_channel::<S::Event>();

        let store = cx.provide_context(ReduxStore {
            inner: Rc::new(RefCell::new(create_store())),
            event_dispatcher: event_tx,
            cache_map: Rc::default(),
            schedule_update_any: cx.schedule_update_any(),
        });

        cx.spawn(async move {
            while let Some(event) = event_rx.recv().await {
                store.handle(event)
            }
        });
    });
}

pub fn use_slice<
    'a,
    F: Copy + 'static + Fn(&S) -> T,
    S: 'static + 'a + Store,
    T: 'static + Clone + PartialEq,
>(
    cx: Scope,
    slicer: F,
) -> &ReduxSlice<T> {
    let store = cx.consume_context::<ReduxStore<S>>().unwrap();
    let subscribe = cx.use_hook({
        to_owned![store];
        move || {
            store.subscribe(
                cx.scope_id(),
                TypeId::of::<F>(),
                {
                    to_owned![store];
                    move || {
                        let store = &store.inner.borrow();
                        slicer(store)
                    }
                },
                {
                    to_owned![store];
                    || {
                        Rc::new(RefCell::new(move |cached: &Rc<RefCell<Box<dyn Any>>>| {
                            let store = &store.inner.borrow();
                            let current = slicer(store);
                            let is_same = {
                                let cached = cached.borrow();
                                let cached = cached.downcast_ref::<T>().unwrap();
                                cached == &current
                            };
                            if !is_same {
                                *cached.borrow_mut() = Box::new(current);
                            }
                            is_same
                        }))
                    }
                },
            )
        }
    });
    cx.use_hook(|| ReduxSlice {
        subscribe: Rc::new(subscribe.clone()),
        _phantom: PhantomData::default(),
    })
}

pub struct ReduxSlice<T> {
    subscribe: Rc<Subscribe>,
    _phantom: PhantomData<T>,
}

impl<T: 'static> ReduxSlice<T> {
    pub fn read(&self) -> Rc<RefCell<Box<T>>> {
        let value = self.subscribe.cache.value.clone();
        downcast(value)
    }
}

fn downcast<T: Any>(v: Rc<RefCell<Box<dyn Any>>>) -> Rc<RefCell<Box<T>>> {
    let v: *const RefCell<Box<dyn Any>> = Rc::into_raw(v);
    unsafe { Rc::from_raw(v as *const RefCell<Box<T>>) }
}

#[derive(Clone)]
pub struct ReduxDispatcher<S: Store> {
    event_dispatcher: UnboundedSender<S::Event>,
}

impl<S: Store> ReduxDispatcher<S> {
    pub fn dispatch(&self, event: S::Event) {
        // TODO: Handle errors
        self.event_dispatcher.send(event).unwrap();
    }
}

pub fn use_dispatcher<S: 'static + Store>(cx: Scope) -> ReduxDispatcher<S> {
    let store = cx.consume_context::<ReduxStore<S>>().unwrap();
    ReduxDispatcher {
        event_dispatcher: store.event_dispatcher,
    }
}

pub mod prelude {
    pub use crate::*;
}
