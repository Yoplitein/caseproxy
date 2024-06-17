#![allow(unused, non_snake_case, non_upper_case_globals)]

use std::{cmp::Ordering, collections::VecDeque, ffi::{OsStr, OsString}, fs::read_dir, hash::{DefaultHasher, Hash, Hasher}, ops::{Deref, DerefMut}, path::{Component, Path, PathBuf}};

use anyhow::{anyhow, Ok};
pub use anyhow::Result as AResult;

#[derive(Clone, Debug, Eq)]
pub struct InsensitivePath(pub PathBuf);

impl InsensitivePath {
	pub fn find_matching_files(&self, root: Option<&Path>) -> AResult<Vec<PathBuf>> {
		let root = root.unwrap_or(Path::new("."));
		let mut matchingFiles = Vec::new();
		let mut queue = VecDeque::new();
		queue.push_back((
			PathBuf::from(""),
			if root == Path::new(".") {
				self.to_path_buf()
			} else {
				self.strip_prefix(root)?.to_path_buf()
			}
		));

		while let Some((mut prefix, mut remaining)) = queue.pop_front() {
			let headPath = {
				let mut components = remaining.components();

				let head = components.next();
				let Some(Component::Normal(headPath)) = head else {
					return Err(anyhow!("head of remaining path components is unexpectedly {head:?}"));
				};
				let headPath = headPath.to_os_string();

				remaining = components.collect();

				headPath
			};

			let mut fullPath = PathBuf::new();
			fullPath.push(root);
			fullPath.push(&prefix);
			if remaining.components().next().is_none() {
				// head component is filename
				for entry in read_dir(&fullPath)? {
					let entry = entry?;
					let filename = entry.file_name();
					if compare_osstr_case_insensitive(&filename, &headPath) == Ordering::Equal {
						fullPath.push(filename);
						matchingFiles.push(fullPath.to_path_buf());
						fullPath.pop();
					}
				}
			} else {
				// head component is a directory
				for entry in read_dir(&fullPath)? {
					let entry = entry?;
					if !entry.file_type()?.is_dir() { continue; }

					let filename = entry.file_name();
					if compare_osstr_case_insensitive(&filename, &headPath) == Ordering::Equal {
						let mut relativePath = PathBuf::new();
						relativePath.push(&prefix);
						relativePath.push(filename);
						queue.push_back((relativePath, remaining.clone()));
					}
				}
			}
		}

		Ok(matchingFiles)
	}
}

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
		let mut leftComponents = self.components();
		let mut rightComponents = other.components();
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
							let order = compare_osstr_case_insensitive(l, r);
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

impl Hash for InsensitivePath {
	fn hash<H: Hasher>(&self, state: &mut H) {
		for item in osstr_chars_lowercased(self.0.as_os_str()) {
			match item {
				CharOrByte::Char(char) => state.write_u32(char as u32),
				CharOrByte::Byte(byte) => state.write_u8(byte),
			}
		}
	}
}

#[test]
fn test_insensitive_path() {
	let a = InsensitivePath(PathBuf::from("foo"));
	let b = InsensitivePath(PathBuf::from("Foo"));
	assert_eq!(a, b);

	let aHash = {
		let mut hasher = DefaultHasher::new();
		a.hash(&mut hasher);
		hasher.finish()
	};
	let bHash = {
		let mut hasher = DefaultHasher::new();
		b.hash(&mut hasher);
		hasher.finish()
	};
	assert_eq!(aHash, bHash);

	let a = InsensitivePath(PathBuf::from("abc"));
	let b = InsensitivePath(PathBuf::from("def"));
	assert_ne!(a, b);
	assert!(a < b);
	assert!(b > a);

	let aHash = {
		let mut hasher = DefaultHasher::new();
		a.hash(&mut hasher);
		hasher.finish()
	};
	let bHash = {
		let mut hasher = DefaultHasher::new();
		b.hash(&mut hasher);
		hasher.finish()
	};
	assert_ne!(aHash, bHash);
}

struct Deferred<Func: FnOnce()>(Option<Func>);

impl<Func: FnOnce()> Deferred<Func> {
	fn new(func: Func) -> Self {
		Self(Some(func))
	}
}

impl<Func: FnOnce()> Drop for Deferred<Func> {
	fn drop(&mut self) {
		self.0.take().unwrap()();
	}
}

#[test]
fn test_insensitive_path_searching() -> AResult<()> {
	use rand::{thread_rng, Rng};
	
	let mut tempdir = std::env::temp_dir();
	tempdir.push(&format!("caseproxy_tmp_{:05}", thread_rng().gen::<u16>()));
	let removeTempdir = Deferred::new(|| {
		if let Err(err) = std::fs::remove_dir_all(&tempdir) {
			eprintln!("unable to remove temp directory {tempdir:?}");
		}
	});
	
	let file = |path: &str| -> AResult<()> {
		let fullPath = tempdir.join(path);
		std::fs::create_dir_all(fullPath.parent().unwrap())?;
		std::fs::write(fullPath, "")?;
		Ok(())
	};
	let find = |path: &str| -> AResult<Vec<PathBuf>> {
		let fullPath = tempdir.join(path);
		InsensitivePath(fullPath).find_matching_files(Some(&tempdir))
	};
	
	file("normal.txt");
	assert_eq!(
		find("normal.txt")?,
		vec![
			tempdir.join("normal.txt"),
		]
	);
	
	file("abc.txt");
	file("Abc.txt");
	assert_eq!(
		find("abc.txt")?,
		vec![
			tempdir.join("abc.txt"),
			tempdir.join("Abc.txt"),
		]
	);
	
	file("nested/normal.txt");
	file("nested/abc.txt");
	file("nested/Abc.txt");
	assert_eq!(
		find("nested/normal.txt")?,
		vec![
			tempdir.join("nested/normal.txt"),
		]
	);
	assert_eq!(
		find("nested/abc.txt")?,
		vec![
			tempdir.join("nested/abc.txt"),
			tempdir.join("nested/Abc.txt"),
		]
	);
	
	file("deeply/nested/abc.txt");
	file("deeply/nested/Abc.txt");
	file("deeply/Nested/abc.txt");
	file("deeply/Nested/Abc.txt");
	assert_eq!(
		find("Deeply/Nested/abc.txt")?,
		vec![
			tempdir.join("deeply/nested/abc.txt"),
			tempdir.join("deeply/nested/Abc.txt"),
			tempdir.join("deeply/Nested/abc.txt"),
			tempdir.join("deeply/Nested/Abc.txt"),
		]
	);
	
	Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharOrByte {
    Char(char),
    Byte(u8),
}

pub fn osstr_chars(str: &OsStr) -> impl '_ + Iterator<Item = CharOrByte> {
    let mut index = 0;
    std::iter::from_fn(move || {
        if index >= str.len() {
            return None;
        }

        let headByte = str.as_encoded_bytes()[index];
        let charLen = if headByte & 0b1000_0000 == 0 {
            1
        } else if headByte & 0b1100_0000 == 0b1100_0000 {
            2
        } else if headByte & 0b1110_0000 == 0b1110_0000 {
            3
        } else if headByte & 0b1111_0000 == 0b1111_0000 {
            4
        } else {
            unreachable!()
        };
        if index + charLen > str.len() {
            let byte = str.as_encoded_bytes()[index];
            index += 1;
            return Some(CharOrByte::Byte(byte));
        }
        let slice = &str.as_encoded_bytes()[index .. index + charLen];
        if let std::result::Result::Ok(utf8) = std::str::from_utf8(slice) {
            index += charLen;
            return utf8.chars().next().map(CharOrByte::Char);
        } else {
            let byte = str.as_encoded_bytes()[index];
            index += 1;
            return Some(CharOrByte::Byte(byte));
        }
    })
}

pub fn osstr_chars_lowercased(str: &OsStr) -> impl '_ + Iterator<Item = CharOrByte> {
	osstr_chars(str).flat_map(|v| -> smallvec::SmallVec<[CharOrByte; 16]> {
		match v {
			CharOrByte::Char(c) => c.to_lowercase().map(CharOrByte::Char).collect(),
			_ => smallvec::smallvec![v],
		}
	})
}

#[test]
fn test_osstr_chars() {
	use CharOrByte::*;

    let mut str = OsString::from("ab\u{c9}cd").into_encoded_bytes();
    str.insert(str.len() - 1, b'\xff');
    let str = unsafe {
        OsString::from_encoded_bytes_unchecked(str)
    };
	let chars: Vec<_> = osstr_chars(&str).collect();
    assert_eq!(
		chars,
		vec![
			Char('a'),
			Char('b'),
			Char('\u{c9}'),
			Char('c'),
			Byte(b'\xff'),
			Char('d'),
		]
	);

	let str = OsString::from("Ab");
	let chars: Vec<_> = osstr_chars_lowercased(&str).collect();
	assert_eq!(
		chars,
		vec![
			Char('a'),
			Char('b'),
		]
	);
}

fn compare_osstr_case_insensitive(left: &OsStr, right: &OsStr) -> Ordering {
	let mut left = osstr_chars_lowercased(left);
	let mut right = osstr_chars_lowercased(right);
	loop {
		let pair = (left.next(), right.next());
		match pair {
			(None, Some(_)) => return Ordering::Less,
			(Some(_), None) => return Ordering::Greater,
			(None, None) => return Ordering::Equal,
			(Some(l), Some(r)) => {
				use CharOrByte::*;
				match (l, r) {
					(Char(l), Char(r)) => {
						let order = l.cmp(&r);
						if order != Ordering::Equal {
							return order;
						}
					},
					(Byte(l), Byte(r)) => {
						let order = l.cmp(&r);
						if order != Ordering::Equal {
							return order;
						}
					},
					(Char(_), Byte(_)) => {
						return Ordering::Less;
					},
					(Byte(_), Char(_)) => {
						return Ordering::Greater;
					},
				}
			},
		}
	}
}

#[test]
fn test_osstr_case_insensitive() {
	let a = OsString::from("foo");
	let b = OsString::from("Foo");
	assert_eq!(compare_osstr_case_insensitive(&a, &b), Ordering::Equal);

	let a = OsString::from("abc");
	let b = OsString::from("def");
	assert_eq!(compare_osstr_case_insensitive(&a, &b), Ordering::Less);
	assert_eq!(compare_osstr_case_insensitive(&b, &a), Ordering::Greater);
}

pub fn resolve_parents(path: &Path) -> PathBuf {
	let mut res = PathBuf::new();
	for component in path.components() {
		if (
			component == Component::ParentDir &&
			res != Path::new("/") &&
			res != Path::new(".")
		) {
			res.pop();
		} else {
			res.push(component);
		}
	}
	res
}

#[test]
fn test_resolve_parents() {
	assert_eq!(
		resolve_parents(Path::new("foo")),
		Path::new("foo")
	);
	assert_eq!(
		resolve_parents(Path::new("./foo")),
		Path::new("./foo")
	);
	assert_eq!(
		resolve_parents(Path::new("/foo")),
		Path::new("/foo")
	);
	assert_eq!(
		resolve_parents(Path::new("foo/")),
		Path::new("foo/")
	);
	assert_eq!(
		resolve_parents(Path::new("./foo/")),
		Path::new("./foo/")
	);
	assert_eq!(
		resolve_parents(Path::new("/foo/")),
		Path::new("/foo/")
	);
	
	assert_eq!(
		resolve_parents(Path::new("foo/bar")),
		Path::new("foo/bar")
	);
	assert_eq!(
		resolve_parents(Path::new("./foo/bar")),
		Path::new("./foo/bar")
	);
	assert_eq!(
		resolve_parents(Path::new("/foo/bar")),
		Path::new("/foo/bar")
	);
	assert_eq!(
		resolve_parents(Path::new("foo/bar/")),
		Path::new("foo/bar/")
	);
	assert_eq!(
		resolve_parents(Path::new("./foo/bar/")),
		Path::new("./foo/bar/")
	);
	assert_eq!(
		resolve_parents(Path::new("/foo/bar/")),
		Path::new("/foo/bar/")
	);
	
	assert_eq!(
		resolve_parents(Path::new("foo/bar/..")),
		Path::new("foo")
	);
	assert_eq!(
		resolve_parents(Path::new("./foo/bar/..")),
		Path::new("./foo")
	);
	assert_eq!(
		resolve_parents(Path::new("/foo/bar/..")),
		Path::new("/foo")
	);
	assert_eq!(
		resolve_parents(Path::new("foo/bar/../")),
		Path::new("foo")
	);
	assert_eq!(
		resolve_parents(Path::new("./foo/bar/../")),
		Path::new("./foo")
	);
	assert_eq!(
		resolve_parents(Path::new("/foo/bar/../")),
		Path::new("/foo")
	);
	
	assert_eq!(
		resolve_parents(Path::new("foo/../bar")),
		Path::new("bar")
	);
	assert_eq!(
		resolve_parents(Path::new("./foo/../bar")),
		Path::new("./bar")
	);
	assert_eq!(
		resolve_parents(Path::new("/foo/../bar")),
		Path::new("/bar")
	);
	assert_eq!(
		resolve_parents(Path::new("foo/../bar/")),
		Path::new("bar")
	);
	assert_eq!(
		resolve_parents(Path::new("./foo/../bar/")),
		Path::new("./bar")
	);
	assert_eq!(
		resolve_parents(Path::new("/foo/../bar/")),
		Path::new("/bar")
	);
	
	assert_eq!(
		resolve_parents(Path::new("foo/bar/../..")),
		Path::new("")
	);
	assert_eq!(
		resolve_parents(Path::new("./foo/bar/../..")),
		Path::new(".")
	);
	assert_eq!(
		resolve_parents(Path::new("/foo/bar/../..")),
		Path::new("/")
	);
	assert_eq!(
		resolve_parents(Path::new("foo/bar/../../..")),
		Path::new("")
	);
	assert_eq!(
		resolve_parents(Path::new("./foo/bar/../../..")),
		Path::new(".")
	);
	assert_eq!(
		resolve_parents(Path::new("/foo/bar/../../..")),
		Path::new("/")
	);
}
