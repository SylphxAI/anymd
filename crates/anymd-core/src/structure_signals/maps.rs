use std::collections::{HashMap, HashSet, VecDeque};

use lopdf::{Dictionary, Object};

use super::walk::{object_list, Walker, MAX_ARRAY_ITEMS, PDFJS_MAX_DEPTH};

pub(super) fn read_role_map<'a>(
    walker: &mut Walker<'a, '_>,
    root: &'a Dictionary,
) -> HashMap<Vec<u8>, Vec<u8>> {
    let Some(dict) = root
        .get(b"RoleMap")
        .ok()
        .and_then(|value| walker.dict(value))
    else {
        return HashMap::new();
    };
    if walker.admit_items(dict.len()).is_none() {
        return HashMap::new();
    }
    let mut output = HashMap::new();
    for (key, value) in dict.iter() {
        let Some(value) = walker.resolve(value).and_then(|value| value.as_name().ok()) else {
            continue;
        };
        if walker
            .admit_text(key.len().saturating_add(value.len()))
            .is_none()
        {
            return HashMap::new();
        }
        output.insert(key.clone(), value.to_vec());
        walker.materialized(1);
    }
    output
}

pub(super) fn read_number_tree<'a>(
    walker: &mut Walker<'a, '_>,
    root: &'a Object,
) -> Option<HashMap<i64, &'a Object>> {
    walker.admit_work()?;
    let mut queue = VecDeque::from([(root, 0usize)]);
    walker.materialized(1);
    let mut seen = HashSet::new();
    let mut output = HashMap::new();
    while let Some((value, depth)) = queue.pop_front() {
        if depth > PDFJS_MAX_DEPTH || output.len() > MAX_ARRAY_ITEMS {
            walker.limited = true;
            return None;
        }
        if let Ok(id) = value.as_reference() {
            if !seen.insert(id) {
                walker.failed = true;
                return None;
            }
        }
        if depth > 0 {
            walker.admit_work()?;
        }
        let dict = walker.dict(value)?;
        if let Ok(kids) = dict.get(b"Kids") {
            for kid in object_list(walker, kids).ok()? {
                queue.push_back((kid, depth + 1));
            }
            continue;
        }
        if let Ok(nums) = dict.get(b"Nums") {
            let nums = object_list(walker, nums).ok()?;
            if nums.len() % 2 != 0 || output.len() + nums.len() / 2 > MAX_ARRAY_ITEMS {
                walker.limited = true;
                return None;
            }
            for pair in nums.as_chunks::<2>().0.iter() {
                if let Some(key) = walker.integer(pair[0]) {
                    output.insert(key, pair[1]);
                }
            }
        }
    }
    Some(output)
}
