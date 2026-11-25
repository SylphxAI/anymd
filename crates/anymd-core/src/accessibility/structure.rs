use serde_json::Value;

#[derive(Clone, Copy, Default)]
pub(super) struct RoleStats {
    pub(super) roles: usize,
    pub(super) contents: usize,
    pub(super) content_ids: usize,
    pub(super) headings: usize,
    pub(super) figures: usize,
}

impl RoleStats {
    fn add(&mut self, other: Self) {
        self.roles += other.roles;
        self.contents += other.contents;
        self.content_ids += other.content_ids;
        self.headings += other.headings;
        self.figures += other.figures;
    }
}

pub(super) fn page_role_stats(trees: &[Value], page: u32) -> Option<RoleStats> {
    trees
        .iter()
        .find(|entry| entry.get("page").and_then(Value::as_u64) == Some(u64::from(page)))
        .map(|entry| entry.get("tree").map(count_roles).unwrap_or_default())
}

fn count_roles(node: &Value) -> RoleStats {
    let role = node
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let mut stats = RoleStats {
        roles: 1,
        headings: usize::from(
            matches!(role.as_str(), "h" | "heading")
                || role.len() == 2
                    && role.starts_with('h')
                    && role.as_bytes()[1].is_ascii_digit()
                    && (b'1'..=b'6').contains(&role.as_bytes()[1]),
        ),
        figures: usize::from(role == "figure"),
        ..RoleStats::default()
    };
    for child in node
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if child.get("role").is_some() {
            stats.add(count_roles(child));
        } else {
            stats.contents += 1;
            stats.content_ids += usize::from(child.get("id").is_some());
        }
    }
    stats
}

pub(super) fn mark_bool(value: Option<&Value>, key: &str) -> Option<bool> {
    value
        .and_then(|v| v.get(key).or_else(|| v.get(key.to_ascii_lowercase())))
        .and_then(Value::as_bool)
}

pub(super) fn count_outline(value: Option<&Value>) -> usize {
    fn walk(items: &[Value]) -> usize {
        items
            .iter()
            .map(|i| {
                1 + i
                    .get("items")
                    .and_then(Value::as_array)
                    .map_or(0, |a| walk(a))
            })
            .sum()
    }
    value.and_then(Value::as_array).map_or(0, |a| walk(a))
}
