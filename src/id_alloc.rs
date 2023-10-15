/// Taken from Bevy (will put a better notice later on)
use std::{convert::TryFrom, fmt, sync::atomic::Ordering};

#[cfg(target_has_atomic = "64")]
use std::sync::atomic::AtomicI64 as AtomicIdCursor;
#[cfg(target_has_atomic = "64")]
type IdCursor = i64;

/// Most modern platforms support 64-bit atomics, but some less-common platforms
/// do not. This fallback allows compilation using a 32-bit cursor instead, with
/// the caveat that some conversions may fail (and panic) at runtime.
#[cfg(not(target_has_atomic = "64"))]
use std::sync::atomic::AtomicIsize as AtomicIdCursor;
#[cfg(not(target_has_atomic = "64"))]
type IdCursor = isize;

/// A unique, reusable identifier.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Id {
    generation: u32,
    index: u32,
}

impl Id {
    /// An placeholder ID. This may or may not correspond to an allocated ID,
    /// and should be overwritten by a new value before being used.
    pub const PLACEHOLDER: Self = Self::from_raw(u32::MAX);

    /// Creates a new ID with the specified `index` and a generation of 0.
    ///
    /// # Note
    ///
    /// Spawning a specific `id` value is __rarely the right choice__. Most apps should favor
    /// [`Ids::allocate`]. This method should generally
    /// only be used for sharing entities across apps, and only when they have a scheme
    /// worked out to share an index space (which doesn't happen by default).
    ///
    /// In general, one should not try to synchronize the ECS by attempting to ensure that
    /// `Entity` lines up between instances, but instead insert a secondary identifier as
    /// a component.
    pub const fn from_raw(index: u32) -> Id {
        Id {
            index,
            generation: 0,
        }
    }

    /// Convert to a form convenient for passing outside of rust.
    ///
    /// Only useful for identifying entities within the same instance of an application. Do not use
    /// for serialization between runs.
    ///
    /// No particular structure is guaranteed for the returned bits.
    pub const fn to_bits(self) -> u64 {
        (self.generation as u64) << 32 | self.index as u64
    }

    /// Reconstruct an `Entity` previously destructured with [`Entity::to_bits`].
    ///
    /// Only useful when applied to results from `to_bits` in the same instance of an application.
    pub const fn from_bits(bits: u64) -> Self {
        Self {
            generation: (bits >> 32) as u32,
            index: bits as u32,
        }
    }

    /// Return a transiently unique identifier.
    ///
    /// No two simultaneously-live entities share the same index, but dead entities' indices may collide
    /// with both live and dead entities. Useful for compactly representing entities within a
    /// specific snapshot of the world, such as when serializing.
    #[inline]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Returns the generation of this Entity's index. The generation is incremented each time an
    /// entity with a given index is despawned. This serves as a "count" of the number of times a
    /// given index has been reused (index, generation) pairs uniquely identify a given Entity.
    #[inline]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}v{}", self.index, self.generation)
    }
}

/// An [`Iterator`] returning a sequence of [`Id`] values from
/// [`Entities::reserve_entities`].
pub struct ReserveEntitiesIterator<'a> {
    // Metas, so we can recover the current generation for anything in the freelist.
    generations: &'a [Generation],

    // Reserved indices formerly in the freelist to hand out.
    index_iter: std::slice::Iter<'a, u32>,

    // New Entity indices to hand out, outside the range of meta.len().
    index_range: std::ops::Range<u32>,
}

impl<'a> Iterator for ReserveEntitiesIterator<'a> {
    type Item = Id;

    fn next(&mut self) -> Option<Self::Item> {
        self.index_iter
            .next()
            .map(|&index| Id {
                generation: self.generations[index as usize].generation,
                index,
            })
            .or_else(|| {
                self.index_range.next().map(|index| Id {
                    generation: 0,
                    index,
                })
            })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.index_iter.len() + self.index_range.len();
        (len, Some(len))
    }
}

impl<'a> core::iter::ExactSizeIterator for ReserveEntitiesIterator<'a> {}
impl<'a> core::iter::FusedIterator for ReserveEntitiesIterator<'a> {}

/// A [`World`]'s internal metadata store on all of its entities.
///
/// Contains metadata on:
///  - The generation of every entity.
///  - The alive/dead status of a particular entity. (i.e. "has entity 3 been despawned?")
///  - The location of the entity's components in memory (via [`EntityLocation`])
///
/// [`World`]: crate::world::World
#[derive(Debug, Default)]
pub struct Ids {
    meta: Vec<Generation>,

    /// The `pending` and `free_cursor` fields describe three sets of Entity IDs
    /// that have been freed or are in the process of being allocated:
    ///
    /// - The `freelist` IDs, previously freed by `free()`. These IDs are available to any of
    ///   [`alloc`], [`reserve_entity`] or [`reserve_entities`]. Allocation will always prefer
    ///   these over brand new IDs.
    ///
    /// - The `reserved` list of IDs that were once in the freelist, but got reserved by
    ///   [`reserve_entities`] or [`reserve_entity`]. They are now waiting for [`flush`] to make them
    ///   fully allocated.
    ///
    /// - The count of new IDs that do not yet exist in `self.meta`, but which we have handed out
    ///   and reserved. [`flush`] will allocate room for them in `self.meta`.
    ///
    /// The contents of `pending` look like this:
    ///
    /// ```txt
    /// ----------------------------
    /// |  freelist  |  reserved   |
    /// ----------------------------
    ///              ^             ^
    ///          free_cursor   pending.len()
    /// ```
    ///
    /// As IDs are allocated, `free_cursor` is atomically decremented, moving
    /// items from the freelist into the reserved list by sliding over the boundary.
    ///
    /// Once the freelist runs out, `free_cursor` starts going negative.
    /// The more negative it is, the more IDs have been reserved starting exactly at
    /// the end of `meta.len()`.
    ///
    /// This formulation allows us to reserve any number of IDs first from the freelist
    /// and then from the new IDs, using only a single atomic subtract.
    ///
    /// Once [`flush`] is done, `free_cursor` will equal `pending.len()`.
    ///
    /// [`alloc`]: Entities::alloc
    /// [`reserve_entity`]: Entities::reserve_entity
    /// [`reserve_entities`]: Entities::reserve_entities
    /// [`flush`]: Entities::flush
    pending: Vec<u32>,
    free_cursor: AtomicIdCursor,
    /// Stores the number of free entities for [`len`](Entities::len)
    len: u32,
}

impl Ids {
    pub const fn new() -> Self {
        Ids {
            meta: Vec::new(),
            pending: Vec::new(),
            free_cursor: AtomicIdCursor::new(0),
            len: 0,
        }
    }

    /// Reserve entity IDs concurrently.
    ///
    /// Storage for entity generation and location is lazily allocated by calling [`flush`](Entities::flush).
    pub fn allocate_ids(&self, count: u32) -> ReserveEntitiesIterator {
        // Use one atomic subtract to grab a range of new IDs. The range might be
        // entirely nonnegative, meaning all IDs come from the freelist, or entirely
        // negative, meaning they are all new IDs to allocate, or a mix of both.
        let range_end = self
            .free_cursor
            // Unwrap: these conversions can only fail on platforms that don't support 64-bit atomics
            // and use AtomicIsize instead (see note on `IdCursor`).
            .fetch_sub(IdCursor::try_from(count).unwrap(), Ordering::Relaxed);
        let range_start = range_end - IdCursor::try_from(count).unwrap();

        let freelist_range = range_start.max(0) as usize..range_end.max(0) as usize;

        let (new_id_start, new_id_end) = if range_start >= 0 {
            // We satisfied all requests from the freelist.
            (0, 0)
        } else {
            // We need to allocate some new Entity IDs outside of the range of self.meta.
            //
            // `range_start` covers some negative territory, e.g. `-3..6`.
            // Since the nonnegative values `0..6` are handled by the freelist, that
            // means we need to handle the negative range here.
            //
            // In this example, we truncate the end to 0, leaving us with `-3..0`.
            // Then we negate these values to indicate how far beyond the end of `meta.end()`
            // to go, yielding `meta.len()+0 .. meta.len()+3`.
            let base = self.meta.len() as IdCursor;

            let new_id_end = u32::try_from(base - range_start).expect("too many entities");

            // `new_id_end` is in range, so no need to check `start`.
            let new_id_start = (base - range_end.min(0)) as u32;

            (new_id_start, new_id_end)
        };

        ReserveEntitiesIterator {
            generations: &self.meta[..],
            index_iter: self.pending[freelist_range].iter(),
            index_range: new_id_start..new_id_end,
        }
    }

    /// Reserve one entity ID concurrently.
    ///
    /// Equivalent to `self.reserve_entities(1).next().unwrap()`, but more efficient.
    pub fn allocate_id(&self) -> Id {
        let n = self.free_cursor.fetch_sub(1, Ordering::Relaxed);
        if n > 0 {
            // Allocate from the freelist.
            let index = self.pending[(n - 1) as usize];
            Id {
                generation: self.meta[index as usize].generation,
                index,
            }
        } else {
            // Grab a new ID, outside the range of `meta.len()`. `flush()` must
            // eventually be called to make it valid.
            //
            // As `self.free_cursor` goes more and more negative, we return IDs farther
            // and farther beyond `meta.len()`.
            Id {
                generation: 0,
                index: u32::try_from(self.meta.len() as IdCursor - n).expect("too many entities"),
            }
        }
    }

    fn flush_if_needed(&mut self) {
        if self.needs_flush() {
            self.flush();
        }
    }

    /// Allocate an entity ID directly.
    pub fn alloc_directly(&mut self) -> Id {
        self.flush_if_needed();
        self.len += 1;
        if let Some(index) = self.pending.pop() {
            let new_free_cursor = self.pending.len() as IdCursor;
            *self.free_cursor.get_mut() = new_free_cursor;
            Id {
                generation: self.meta[index as usize].generation,
                index,
            }
        } else {
            let index = u32::try_from(self.meta.len()).expect("too many entities");
            self.meta.push(Generation::EMPTY);
            Id {
                generation: 0,
                index,
            }
        }
    }

    /// Destroy an entity, allowing it to be reused.
    ///
    /// Must not be called while reserved entities are awaiting `flush()`.
    pub fn free(&mut self, id: Id) -> bool {
        self.flush_if_needed();

        let meta = &mut self.meta[id.index as usize];
        if meta.generation != id.generation {
            return false;
        }
        meta.generation += 1;

        self.pending.push(id.index);

        let new_free_cursor = self.pending.len() as IdCursor;
        *self.free_cursor.get_mut() = new_free_cursor;
        self.len -= 1;
        true
    }

    /// Ensure at least `n` allocations can succeed without reallocating.
    pub fn reserve(&mut self, additional: u32) {
        self.flush_if_needed();

        let freelist_size = *self.free_cursor.get_mut();
        // Unwrap: these conversions can only fail on platforms that don't support 64-bit atomics
        // and use AtomicIsize instead (see note on `IdCursor`).
        let shortfall = IdCursor::try_from(additional).unwrap() - freelist_size;
        if shortfall > 0 {
            self.meta.reserve(shortfall as usize);
        }
    }

    /// Returns true if the [`Entities`] contains [`entity`](Entity).
    // This will return false for entities which have been freed, even if
    // not reallocated since the generation is incremented in `free`
    pub fn contains(&self, id: Id) -> bool {
        self.resolve_from_id(id.index())
            .map_or(false, |e| e.generation() == id.generation)
    }

    /// Clears all [`Entity`] from the World.
    pub fn clear(&mut self) {
        self.meta.clear();
        self.pending.clear();
        *self.free_cursor.get_mut() = 0;
        self.len = 0;
    }

    /// Get the [`Entity`] with a given id, if it exists in this [`Entities`] collection
    /// Returns `None` if this [`Entity`] is outside of the range of currently reserved Entities
    ///
    /// Note: This method may return [`Entities`](Entity) which are currently free
    /// Note that [`contains`](Entities::contains) will correctly return false for freed
    /// entities, since it checks the generation
    pub fn resolve_from_id(&self, index: u32) -> Option<Id> {
        let idu = index as usize;
        if let Some(&Generation { generation, .. }) = self.meta.get(idu) {
            Some(Id { generation, index })
        } else {
            // `id` is outside of the meta list - check whether it is reserved but not yet flushed.
            let free_cursor = self.free_cursor.load(Ordering::Relaxed);
            // If this entity was manually created, then free_cursor might be positive
            // Returning None handles that case correctly
            let num_pending = usize::try_from(-free_cursor).ok()?;
            (idu < self.meta.len() + num_pending).then_some(Id {
                generation: 0,
                index,
            })
        }
    }

    fn needs_flush(&mut self) -> bool {
        *self.free_cursor.get_mut() != self.pending.len() as IdCursor
    }

    /// Allocates space for entities previously reserved with [`reserve_entity`](Entities::reserve_entity) or
    /// [`reserve_entities`](Entities::reserve_entities), then initializes each one using the supplied function.
    pub fn flush(&mut self) {
        let free_cursor = self.free_cursor.get_mut();
        let current_free_cursor = *free_cursor;

        let new_free_cursor = if current_free_cursor >= 0 {
            current_free_cursor as usize
        } else {
            let old_meta_len = self.meta.len();
            let new_meta_len = old_meta_len + -current_free_cursor as usize;
            self.meta.resize(new_meta_len, Generation::EMPTY);
            self.len += -current_free_cursor as u32;

            *free_cursor = 0;
            0
        };

        self.len += (self.pending.len() - new_free_cursor) as u32;
        self.pending.drain(new_free_cursor..);
    }

    /// The count of all entities in the [`World`] that have ever been allocated
    /// including the entities that are currently freed.
    ///
    /// This does not include entities that have been reserved but have never been
    /// allocated yet.
    ///
    /// [`World`]: crate::world::World
    #[inline]
    pub fn total_count(&self) -> usize {
        self.meta.len()
    }

    /// The count of currently allocated entities.
    #[inline]
    pub fn len(&self) -> u32 {
        self.len
    }

    /// Checks if any entity is currently active.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

// This type is repr(C) to ensure that the layout and values within it can be safe to fully fill
// with u8::MAX, as required by [`Entities::flush_and_reserve_invalid_assuming_no_entities`].
// Safety:
// This type must not contain any pointers at any level, and be safe to fully fill with u8::MAX.
/// Metadata for an [`Entity`].
#[derive(Copy, Clone, Debug)]
#[repr(C)]
struct Generation {
    /// The current generation of the [`Entity`].
    pub generation: u32,
}

impl Generation {
    /// meta for **pending entity**
    const EMPTY: Generation = Generation { generation: 0 };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_bits_roundtrip() {
        let e = Id {
            generation: 0xDEADBEEF,
            index: 0xBAADF00D,
        };
        assert_eq!(Id::from_bits(e.to_bits()), e);
    }

    #[test]
    fn reserve_entity_len() {
        let mut e = Ids::new();
        e.allocate_id();
        // SAFETY: entity_location is left invalid
        e.flush();
        assert_eq!(e.len(), 1);
    }

    #[test]
    fn get_reserved_and_invalid() {
        let mut entities = Ids::new();
        let e = entities.allocate_id();
        assert!(entities.contains(e));

        entities.flush();

        assert!(entities.contains(e));
    }

    #[test]
    fn entity_const() {
        const C1: Id = Id::from_raw(42);
        assert_eq!(42, C1.index);
        assert_eq!(0, C1.generation);

        const C2: Id = Id::from_bits(0x0000_00ff_0000_00cc);
        assert_eq!(0x0000_00cc, C2.index);
        assert_eq!(0x0000_00ff, C2.generation);

        const C3: u32 = Id::from_raw(33).index();
        assert_eq!(33, C3);

        const C4: u32 = Id::from_bits(0x00dd_00ff_0000_0000).generation();
        assert_eq!(0x00dd_00ff, C4);
    }
}
