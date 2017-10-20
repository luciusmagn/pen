use std::iter;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::collections::hash_map;

type MultiDictListIter<'a, T> = hash_map::Iter<'a, String, Vec<T>>;
type MultiDictListValues<'a, T> = hash_map::Values<'a, String, Vec<T>>;

pub struct MultiDictValues<'a, T: 'a> {
    inner: iter::Map<MultiDictListValues<'a, T>, fn(&'a Vec<T>) -> &'a T>
}

impl<'a, T: 'a> iter::Iterator for MultiDictValues<'a, T> {
    type Item = &'a T;
    #[inline] fn next(&mut self) -> Option<&'a T> { self.inner.next() }
    #[inline] fn size_hint(&self) -> (usize, Option<usize>) { self.inner.size_hint() }
}

pub struct MultiDictIter<'a, T: 'a> {
    inner: iter::Map<MultiDictListIter<'a, T>, for<'b, 'c> fn((&'b String, &'c Vec<T>)) -> (&'b String, &'c T)>
}

impl<'a, T: 'a> iter::Iterator for MultiDictIter<'a, T> {
    type Item = (&'a String, &'a T);
    #[inline] fn next(&mut self) -> Option<(&'a String, &'a T)> { self.inner.next() }
    #[inline] fn size_hint(&self) -> (usize, Option<usize>) { self.inner.size_hint() }
}

#[derive(Clone, Debug)]
pub struct MultiDict<T> {
    map: HashMap<String, Vec<T>>,
}

impl<T> MultiDict<T> {
    pub fn new() -> MultiDict<T> {
        MultiDict {
            map: HashMap::new(),
        }
    }

    pub fn get<B>(&self, key: &str) -> Option<&B>
        where T: Borrow<B>,
              B: ?Sized
    {
        match self.map.get(key) {
            Some(value) => Some(value[0].borrow()),
            None => None
        }
    }

    pub fn set(&mut self, key: &str, value: T) {
        self.map.insert(key.to_owned(), vec![value]);
    }

    pub fn add(&mut self, key: String, value: T) {
        match self.map.remove(&key) {
            Some(mut v) => {
                v.push(value);
                self.map.insert(key, v)
            },
            None => self.map.insert(key, vec![value]),
        };
    }

    pub fn getlist(&self, key: &str) -> Option<&Vec<T>> {
        self.map.get(key)
    }
    
    pub fn iter(&self) -> MultiDictIter<T> {
        fn first<'a, 'b, A, B>(kvpair: (&'a A, &'b Vec<B>)) -> (&'a A, &'b B) { (kvpair.0, &kvpair.1[0]) }
        MultiDictIter { inner: self.listiter().map(first) }
    }

    pub fn listiter(&self) -> hash_map::Iter<String, Vec<T>> {
        self.map.iter()
    }

    pub fn keys(&self) -> hash_map::Keys<String, Vec<T>> {
        self.map.keys()
    }

    pub fn values(&self) -> MultiDictValues<T> {
        #[allow(ptr_arg)]
        fn first<A>(list: &Vec<A>) -> &A { &list[0] }
        MultiDictValues { inner: self.listvalues().map(first) }
    }

    pub fn listvalues(&self) -> hash_map::Values<String, Vec<T>> {
        self.map.values()
    }
}
