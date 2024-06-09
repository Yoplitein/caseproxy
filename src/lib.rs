#![allow(unused, non_snake_case)]

use std::{cmp::Ordering, ops::{Deref, DerefMut}, path::{Component, PathBuf}};

#[derive(Clone, Debug, Eq)]
pub struct InsensitivePath(pub PathBuf);

impl Deref for InsensitivePath {
	type Target = PathBuf;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl DerefMut for InsensitivePath {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}

impl PartialEq for InsensitivePath {
	fn eq(&self, other: &Self) -> bool {
		self.cmp(other) == Ordering::Equal
	}
}

impl PartialOrd for InsensitivePath {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl Ord for InsensitivePath {
	fn cmp(&self, other: &Self) -> Ordering {
		// self.components().zip(other.components())
		let mut leftComponents = self.components();
		let mut rightComponents = other.components();
		let mut lbuf = String::new();
		let mut rbuf = String::new();
		loop {
			let it = (leftComponents.next(), rightComponents.next());
			match it {
				(None, Some(_)) => return Ordering::Less,
				(Some(_), None) => return Ordering::Greater,
				(None, None) => return Ordering::Equal,
				(Some(l), Some(r)) => {
					match (l, r) {
						(Component::Normal(l), Component::Normal(r)) => {
							lbuf.clear();
							let lstr = std::str::from_utf8(l.as_encoded_bytes()).unwrap_or_else(|_|
								panic!("file path {:?} contains non-UTF-8 bytes", self.0)
							);
							for char in lstr.chars() {
								lbuf.extend(char.to_lowercase());
							}
							
							rbuf.clear();
							let rstr = std::str::from_utf8(r.as_encoded_bytes()).unwrap_or_else(|_|
								panic!("file path {:?} contains non-UTF-8 bytes", other.0)
							);
							for char in rstr.chars() {
								rbuf.extend(char.to_lowercase());
							}
							
							let order = lbuf.cmp(&rbuf);
							if order != Ordering::Equal {
								return order;
							}
						},
						_ => {
							let order = l.cmp(&r);
							if order != Ordering::Equal {
								return order;
							}
						}
					}
				}
			}
		}
	}
}

#[test]
fn test_insensitive_path() {
	let a = InsensitivePath(PathBuf::from("foo"));
	let b = InsensitivePath(PathBuf::from("Foo"));
	assert_eq!(a, b);
	
	let a = InsensitivePath(PathBuf::from("abc"));
	let b = InsensitivePath(PathBuf::from("def"));
	assert_ne!(a, b);
	assert!(a < b);
	assert!(b > a);
}
