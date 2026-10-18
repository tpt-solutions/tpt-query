//! Local stub of the entity data model (ids, values, fields, relations) and the
//! in-memory reference source. Swap for `tpt-uir` / `tpt-model` once they exist.

use alloc::{
    boxed::Box,
    collections::{BTreeMap, BTreeSet},
    string::{String, ToString},
    vec::Vec,
};
use core::{cmp::Ordering, fmt};

use crate::{
    ast::Condition,
    engine::QueryError,
    index::{Index, IndexKind, ValueIndex},
};

/// Identifier of an entity. Sources iterate entities in ascending id order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct EntityId(pub u64);

/// Identifier of an entity type, e.g. "CadPart" or "TextRun".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct EntityTypeId(pub u32);

/// A dotted path into an entity's fields, e.g. `"material.name"`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(from = "String", into = "String"))]
pub struct FieldPath {
    segments: Vec<String>,
}

impl FieldPath {
    /// Splits `path` on `.`. Empty segments are kept so validation can reject them.
    ///
    /// A backslash escapes the next character, so `a\.b` is one segment `a.b`, and
    /// `a\\b` is one segment `a\b`. [`Display`](fmt::Display) escapes the same way, so
    /// parsing the text of a path gives the path back.
    pub fn parse(path: &str) -> Self {
        let mut segments = Vec::new();
        let mut current = String::new();
        let mut chars = path.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some(escaped) => current.push(escaped),
                    None => current.push('\\'),
                },
                '.' => segments.push(core::mem::take(&mut current)),
                _ => current.push(c),
            }
        }
        segments.push(current);
        Self { segments }
    }

    /// The path segments in order.
    pub fn segments(&self) -> &[String] {
        &self.segments
    }
}

impl From<&str> for FieldPath {
    fn from(path: &str) -> Self {
        Self::parse(path)
    }
}

impl From<String> for FieldPath {
    fn from(path: String) -> Self {
        Self::parse(&path)
    }
}

impl From<FieldPath> for String {
    fn from(path: FieldPath) -> Self {
        path.to_string()
    }
}

impl fmt::Display for FieldPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, seg) in self.segments.iter().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            for c in seg.chars() {
                if matches!(c, '.' | '\\') {
                    f.write_str("\\")?;
                }
                write!(f, "{c}")?;
            }
        }
        Ok(())
    }
}

/// Axis-aligned bounding box in 3D. 2D data uses `z = 0`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Aabb {
    /// Minimum corner.
    pub min: [f64; 3],
    /// Maximum corner.
    pub max: [f64; 3],
}

impl Aabb {
    /// The empty box: intersects nothing and is the identity for [`Aabb::union`].
    pub const EMPTY: Aabb = Aabb {
        min: [f64::INFINITY; 3],
        max: [f64::NEG_INFINITY; 3],
    };

    /// Creates a box from its corners.
    pub const fn new(min: [f64; 3], max: [f64; 3]) -> Self {
        Self { min, max }
    }

    /// Closed-interval intersection test: touching boxes intersect.
    pub fn intersects(&self, other: &Aabb) -> bool {
        (0..3).all(|i| self.min[i] <= other.max[i] && other.min[i] <= self.max[i])
    }

    /// Smallest box containing both inputs.
    pub fn union(&self, other: &Aabb) -> Aabb {
        let mut min = self.min;
        let mut max = self.max;
        for i in 0..3 {
            min[i] = min[i].min(other.min[i]);
            max[i] = max[i].max(other.max[i]);
        }
        Aabb { min, max }
    }

    /// Sum of edge lengths. Used as the split/insert cost because it stays
    /// non-zero for point-like boxes, unlike volume.
    pub fn margin(&self) -> f64 {
        (0..3).map(|i| self.max[i] - self.min[i]).sum()
    }

    fn cmp_total(&self, other: &Aabb) -> Ordering {
        for (a, b) in self
            .min
            .iter()
            .chain(&self.max)
            .zip(other.min.iter().chain(&other.max))
        {
            match a.total_cmp(b) {
                Ordering::Equal => continue,
                ord => return ord,
            }
        }
        Ordering::Equal
    }
}

/// A field value. Values have a total order: variants order by kind first,
/// and floats use `total_cmp`. Comparison operators in
/// queries only compare values of the same kind, so `Int(1)` never equals
/// `Float(1.0)`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Value {
    /// Absent or unknown value.
    Null,
    /// Boolean.
    Bool(bool),
    /// Signed integer.
    Int(i64),
    /// IEEE-754 double.
    Float(f64),
    /// UTF-8 string.
    Str(String),
    /// Bounding box, the target of [`crate::Condition::Intersects`].
    Aabb(Aabb),
    /// Ordered list; supports `Contains`.
    List(Vec<Value>),
    /// Nested fields; enables dotted field paths.
    Map(BTreeMap<String, Value>),
}

impl Value {
    /// Kind ordinal used to keep values of different kinds apart.
    pub(crate) fn rank(&self) -> u8 {
        match self {
            Value::Null => 0,
            Value::Bool(_) => 1,
            Value::Int(_) => 2,
            Value::Float(_) => 3,
            Value::Str(_) => 4,
            Value::Aabb(_) => 5,
            Value::List(_) => 6,
            Value::Map(_) => 7,
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Value {}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Value::Null, Value::Null) => Ordering::Equal,
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            (Value::Int(a), Value::Int(b)) => a.cmp(b),
            (Value::Float(a), Value::Float(b)) => a.total_cmp(b),
            (Value::Str(a), Value::Str(b)) => a.cmp(b),
            (Value::Aabb(a), Value::Aabb(b)) => a.cmp_total(b),
            (Value::List(a), Value::List(b)) => a.cmp(b),
            (Value::Map(a), Value::Map(b)) => a.cmp(b),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

impl Value {
    /// The string, if this is a [`Value::Str`].
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The integer, if this is a [`Value::Int`]. Floats are not converted.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// The float, if this is a [`Value::Float`]. Integers are not converted.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// The boolean, if this is a [`Value::Bool`].
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

impl From<u32> for Value {
    fn from(v: u32) -> Self {
        Value::Int(i64::from(v))
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int(i64::from(v))
    }
}

impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Self {
        Value::List(v)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Str(v.to_string())
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Str(v)
    }
}

impl From<Aabb> for Value {
    fn from(v: Aabb) -> Self {
        Value::Aabb(v)
    }
}

/// A stored entity: id, type, fields, and its outgoing relationships.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    /// Entity id.
    pub id: EntityId,
    /// Entity type.
    pub ty: EntityTypeId,
    /// Field values, addressed by [`FieldPath`].
    pub fields: BTreeMap<String, Value>,
    /// Parent in the containment tree, if any.
    pub parent: Option<EntityId>,
    /// Children in insertion order.
    pub children: Vec<EntityId>,
    /// Named outgoing edges, each in insertion order.
    pub named: BTreeMap<String, Vec<EntityId>>,
}

impl Entity {
    /// Creates an entity with no fields and no relationships.
    pub fn new(id: EntityId, ty: EntityTypeId) -> Self {
        Self {
            id,
            ty,
            fields: BTreeMap::new(),
            parent: None,
            children: Vec::new(),
            named: BTreeMap::new(),
        }
    }

    /// Sets a top-level field. Chain for several fields.
    pub fn with_field(mut self, name: &str, value: impl Into<Value>) -> Self {
        self.fields.insert(name.to_string(), value.into());
        self
    }

    /// Resolves a field path, descending through [`Value::Map`]s.
    pub fn field(&self, path: &FieldPath) -> Option<&Value> {
        resolve(&self.fields, path)
    }
}

/// Resolves `path` against a field map. Returns `None` for any missing segment.
pub(crate) fn resolve<'a>(
    fields: &'a BTreeMap<String, Value>,
    path: &FieldPath,
) -> Option<&'a Value> {
    let mut segs = path.segments().iter();
    let mut cur = fields.get(segs.next()?)?;
    for seg in segs {
        cur = match cur {
            Value::Map(m) => m.get(seg)?,
            // A numeric segment indexes a list, so `items.0` is the first item.
            Value::List(items) => items.get(seg.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

/// Read access to a source's relationships. Every lookup must be O(1) or O(log N).
pub trait RelationMap {
    /// Parent of `id`, if any.
    fn parent(&self, id: EntityId) -> Option<EntityId>;
    /// Children of `id` in insertion order.
    fn children(&self, id: EntityId) -> &[EntityId];
    /// Targets of the named edge `label` from `id`.
    fn named(&self, id: EntityId, label: &str) -> &[EntityId];
}

/// Result of an index lookup: the index used and its candidate ids.
pub struct IndexHit<'a> {
    /// Name of the index that answered.
    pub index: String,
    /// Number of candidate ids, known before they are read.
    pub len: usize,
    /// Candidate ids, ascending and unique. Read lazily from the index.
    pub ids: Box<dyn Iterator<Item = EntityId> + 'a>,
}

/// Something the query engine can read entities from.
pub trait EntitySource {
    /// Looks up one entity by id.
    fn get(&self, id: EntityId) -> Option<&Entity>;

    /// Iterates the ids of all entities of type `ty`, in ascending order.
    fn ids_of_type<'a>(&'a self, ty: EntityTypeId) -> Box<dyn Iterator<Item = EntityId> + 'a>;

    /// The relationship map, or `None` if this source cannot support traversals.
    fn relations(&self) -> Option<&dyn RelationMap> {
        None
    }

    /// Asks the source's indexes for candidates matching `cond`, or `None` if no
    /// index can answer it.
    fn index_lookup<'a>(&'a self, ty: EntityTypeId, cond: &Condition) -> Option<IndexHit<'a>> {
        let _ = (ty, cond);
        None
    }
}

/// In-memory reference source with relation links and secondary indexes.
///
/// Relations are managed with [`MemorySource::link_child`] and
/// [`MemorySource::link_named`]. [`MemorySource::insert`] keeps the existing
/// relations of an entity it replaces. Named edges are stored outgoing only, so
/// removing a target leaves dangling named references, which a traversal reports
/// as [`crate::QueryError::MissingEntity`].
#[derive(Default)]
pub struct MemorySource {
    entities: BTreeMap<EntityId, Entity>,
    by_type: BTreeMap<EntityTypeId, BTreeSet<EntityId>>,
    indexes: Vec<Box<dyn Index>>,
}

impl MemorySource {
    /// Creates an empty source.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored entities.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Whether the source holds no entities.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Inserts or replaces an entity, keeping its indexes in sync. A replaced
    /// entity keeps its existing parent, children, and named edges.
    pub fn insert(&mut self, mut entity: Entity) {
        let id = entity.id;
        if let Some(old) = self.entities.remove(&id) {
            self.unindex(&old);
            if let Some(set) = self.by_type.get_mut(&old.ty) {
                set.remove(&id);
            }
            entity.parent = old.parent;
            entity.children = old.children;
            entity.named = old.named;
        }
        self.by_type.entry(entity.ty).or_default().insert(id);
        self.index(&entity);
        self.entities.insert(id, entity);
    }

    /// Removes an entity, detaching it from its parent and children.
    pub fn remove(&mut self, id: EntityId) -> Option<Entity> {
        let entity = self.entities.remove(&id)?;
        self.unindex(&entity);
        if let Some(set) = self.by_type.get_mut(&entity.ty) {
            set.remove(&id);
        }
        if let Some(parent) = entity.parent
            && let Some(p) = self.entities.get_mut(&parent)
        {
            p.children.retain(|c| *c != id);
        }
        for child in &entity.children {
            if let Some(c) = self.entities.get_mut(child)
                && c.parent == Some(id)
            {
                c.parent = None;
            }
        }
        Some(entity)
    }

    /// Makes `child` a child of `parent`, detaching it from any previous parent.
    /// Returns `false` if either entity is missing or the ids are equal.
    /// Cycles are allowed; traversals detect them.
    pub fn link_child(&mut self, parent: EntityId, child: EntityId) -> bool {
        if parent == child
            || !self.entities.contains_key(&parent)
            || !self.entities.contains_key(&child)
        {
            return false;
        }
        let old_parent = self.entities.get(&child).and_then(|c| c.parent);
        if let Some(old) = old_parent
            && let Some(o) = self.entities.get_mut(&old)
        {
            o.children.retain(|c| *c != child);
        }
        if let Some(c) = self.entities.get_mut(&child) {
            c.parent = Some(parent);
        }
        if let Some(p) = self.entities.get_mut(&parent) {
            p.children.push(child);
        }
        true
    }

    /// Adds the named edge `from --label--> to`. Returns `false` if either entity
    /// is missing. Duplicate edges are ignored.
    pub fn link_named(&mut self, from: EntityId, label: &str, to: EntityId) -> bool {
        if !self.entities.contains_key(&to) {
            return false;
        }
        match self.entities.get_mut(&from) {
            Some(e) => {
                let targets = e.named.entry(label.to_string()).or_default();
                if !targets.contains(&to) {
                    targets.push(to);
                }
                true
            }
            None => false,
        }
    }

    /// Creates a [`ValueIndex`] over `field` for entities of type `ty`, backfilling
    /// existing entities. Fails if an index with this name already exists.
    pub fn create_index(
        &mut self,
        name: &str,
        ty: EntityTypeId,
        field: impl Into<FieldPath>,
        kind: IndexKind,
    ) -> Result<(), QueryError> {
        self.add_index(Box::new(ValueIndex::new(name, ty, field, kind)))
    }

    /// Adds any [`Index`] implementation, backfilling existing entities. Fails if an
    /// index with the same name already exists.
    pub fn add_index(&mut self, mut index: Box<dyn Index>) -> Result<(), QueryError> {
        if self.indexes.iter().any(|i| i.name() == index.name()) {
            return Err(QueryError::DuplicateIndex(String::from(index.name())));
        }
        let ty = index.entity_type();
        for id in self.by_type.get(&ty).into_iter().flatten() {
            if let Some(e) = self.entities.get(id)
                && let Some(v) = resolve(&e.fields, index.field())
            {
                index.on_insert(*id, v);
            }
        }
        self.indexes.push(index);
        Ok(())
    }

    /// Removes the index named `name`. Returns `false` if there is none.
    pub fn drop_index(&mut self, name: &str) -> bool {
        let before = self.indexes.len();
        self.indexes.retain(|i| i.name() != name);
        self.indexes.len() != before
    }

    fn index(&mut self, e: &Entity) {
        for idx in &mut self.indexes {
            if idx.entity_type() == e.ty
                && let Some(v) = resolve(&e.fields, idx.field())
            {
                idx.on_insert(e.id, v);
            }
        }
    }

    fn unindex(&mut self, e: &Entity) {
        for idx in &mut self.indexes {
            if idx.entity_type() == e.ty
                && let Some(v) = resolve(&e.fields, idx.field())
            {
                idx.on_remove(e.id, v);
            }
        }
    }
}

impl EntitySource for MemorySource {
    fn get(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(&id)
    }

    fn ids_of_type<'a>(&'a self, ty: EntityTypeId) -> Box<dyn Iterator<Item = EntityId> + 'a> {
        match self.by_type.get(&ty) {
            Some(set) => Box::new(set.iter().copied()),
            None => Box::new(core::iter::empty()),
        }
    }

    fn relations(&self) -> Option<&dyn RelationMap> {
        Some(self)
    }

    /// Asks every index on `ty` and returns the one with the fewest candidates.
    fn index_lookup<'a>(&'a self, ty: EntityTypeId, cond: &Condition) -> Option<IndexHit<'a>> {
        self.indexes
            .iter()
            .filter(|i| i.entity_type() == ty)
            .filter_map(|i| i.lookup(cond))
            .min_by_key(|hit| hit.len)
    }
}

impl RelationMap for MemorySource {
    fn parent(&self, id: EntityId) -> Option<EntityId> {
        self.entities.get(&id)?.parent
    }

    fn children(&self, id: EntityId) -> &[EntityId] {
        match self.entities.get(&id) {
            Some(e) => e.children.as_slice(),
            None => &[],
        }
    }

    fn named(&self, id: EntityId, label: &str) -> &[EntityId] {
        match self.entities.get(&id).and_then(|e| e.named.get(label)) {
            Some(targets) => targets.as_slice(),
            None => &[],
        }
    }
}

/// A read-only copy of a [`MemorySource`] at one moment. Later writes to the source do
/// not show in it.
///
/// Taking a snapshot copies every entity, so it costs O(N). The copy keeps no indexes,
/// so queries on it scan and traverse, and they return the same rows an indexed query
/// returns on the source. Use it for a long read that must not see concurrent changes.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    entities: BTreeMap<EntityId, Entity>,
    by_type: BTreeMap<EntityTypeId, BTreeSet<EntityId>>,
}

impl MemorySource {
    /// Copies the current entities into a [`Snapshot`]. See the type's docs for the cost.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            entities: self.entities.clone(),
            by_type: self.by_type.clone(),
        }
    }
}

impl EntitySource for Snapshot {
    fn get(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(&id)
    }

    fn ids_of_type<'a>(&'a self, ty: EntityTypeId) -> Box<dyn Iterator<Item = EntityId> + 'a> {
        match self.by_type.get(&ty) {
            Some(set) => Box::new(set.iter().copied()),
            None => Box::new(core::iter::empty()),
        }
    }

    fn relations(&self) -> Option<&dyn RelationMap> {
        Some(self)
    }
}

impl RelationMap for Snapshot {
    fn parent(&self, id: EntityId) -> Option<EntityId> {
        self.entities.get(&id)?.parent
    }

    fn children(&self, id: EntityId) -> &[EntityId] {
        match self.entities.get(&id) {
            Some(e) => e.children.as_slice(),
            None => &[],
        }
    }

    fn named(&self, id: EntityId, label: &str) -> &[EntityId] {
        match self.entities.get(&id).and_then(|e| e.named.get(label)) {
            Some(targets) => targets.as_slice(),
            None => &[],
        }
    }
}
