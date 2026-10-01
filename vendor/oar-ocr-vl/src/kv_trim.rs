//! KV-cache wrapper supporting head-only trimming and gather.
//!
//! Unlike `candle_nn::kv_cache::KvCache` (only `append` / `reset`), this lets
//! callers roll the cache back to an earlier sequence length or keep an
//! arbitrary subset of positions, leaving the rest of the attention path intact.
//!
//! Append uses a fixed-capacity backing tensor and `slice_set`, so speculative
//! verification never copies the accepted history. `kv` contains narrow views
//! over that backing storage and rollback only changes the logical length.

use candle_core::{Result, Tensor};

/// Append-and-trim KV cache.
///
/// `Clone` mirrors `candle_nn::kv_cache::KvCache::Clone`: it produces a
/// shallow copy that shares the same underlying `Tensor` storage. Cheap; only
/// useful for structures that need to derive `Clone` (e.g. GLM-OCR's text
/// model, which is held by value in multiple places).
#[derive(Debug, Clone)]
pub struct TrimmableKvCache {
    /// Concatenation axis (typically `2` for the seq dim of `(B, H, T, D)` tensors).
    cat_dim: usize,
    /// Full-capacity backing storage, allocated lazily once the batch/head
    /// dimensions are known and retained across page-level resets.
    storage: Option<(Tensor, Tensor)>,
    /// Current `(B, H, cur_len, D)` views into `storage`.
    kv: Option<(Tensor, Tensor)>,
    cur_len: usize,
    /// Capacity of `storage` along `cat_dim`, zero until the first `append`
    /// or `initialize_storage` call. `append` grows this organically (like a
    /// `Vec`) instead of jumping straight to `configured_capacity`, so a
    /// cache that never needs a fixed CUDA-graph buffer doesn't pay for one.
    capacity: usize,
    /// Capacity requested via `new()`. Used by `initialize_storage` (CUDA
    /// graph decode paths that need a fixed-size buffer pinned before the
    /// first append) and the `max_seq_len()` accessor; organic growth in
    /// `append` does not preallocate to this size.
    configured_capacity: usize,
}

// `TrimmableKvCache` lives at the crate root so every model's attention path
// can store one. Several of its trim/gather methods (`trim_to`,
// `keep_indices`, `current_seq_len`, `max_seq_len`, `k`, `v`) are not used on
// the baseline decode path; they stay available for external callers without
// triggering dead-code warnings.
#[allow(dead_code)]
impl TrimmableKvCache {
    pub fn new(cat_dim: usize, max_len: usize) -> Self {
        Self {
            cat_dim,
            storage: None,
            kv: None,
            cur_len: 0,
            capacity: 0,
            configured_capacity: max_len,
        }
    }

    /// Append `(k_new, v_new)` into preallocated storage and return current
    /// logical K/V views.
    pub fn append(&mut self, k_new: &Tensor, v_new: &Tensor) -> Result<(Tensor, Tensor)> {
        let new_len = k_new.dim(self.cat_dim)?;
        let reusable = self.storage.as_ref().is_some_and(|(storage_k, storage_v)| {
            storage_k.dtype() == k_new.dtype()
                && storage_v.dtype() == v_new.dtype()
                && storage_k.device().same_device(k_new.device())
                && storage_v.device().same_device(v_new.device())
                && storage_k.dims().len() == k_new.dims().len()
                && storage_k
                    .dims()
                    .iter()
                    .zip(k_new.dims())
                    .enumerate()
                    .all(|(dim, (stored, new))| dim == self.cat_dim || stored == new)
                && storage_v
                    .dims()
                    .iter()
                    .zip(v_new.dims())
                    .enumerate()
                    .all(|(dim, (stored, new))| dim == self.cat_dim || stored == new)
        });
        if self.storage.is_some() && !reusable {
            self.storage = None;
            self.kv = None;
            self.cur_len = 0;
        }
        // Compatibility changes start a new logical cache. Compute this only
        // after the reset above; otherwise the old length becomes a zero-filled
        // prefix in the replacement storage.
        let required = self.cur_len + new_len;
        if self.storage.is_none() {
            // Grow from what's actually needed rather than jumping straight
            // to `configured_capacity`: most callers never trigger a CUDA
            // graph (which instead pins a fixed buffer up front via
            // `initialize_storage`), so a short document should not reserve
            // e.g. 16K tokens of K/V per layer on its first token.
            let mut shape = k_new.dims().to_vec();
            shape[self.cat_dim] = required;
            self.capacity = required;
            self.storage = Some((
                Tensor::zeros(shape.as_slice(), k_new.dtype(), k_new.device())?,
                Tensor::zeros(shape.as_slice(), v_new.dtype(), v_new.device())?,
            ));
        } else if required > self.capacity {
            let grow_by = self.capacity.max(new_len);
            let mut shape = k_new.dims().to_vec();
            shape[self.cat_dim] = grow_by;
            let (old_k, old_v) = self.storage.as_ref().expect("storage initialized");
            let extra_k = Tensor::zeros(shape.as_slice(), k_new.dtype(), k_new.device())?;
            let extra_v = Tensor::zeros(shape.as_slice(), v_new.dtype(), v_new.device())?;
            self.storage = Some((
                Tensor::cat(&[old_k, &extra_k], self.cat_dim)?.contiguous()?,
                Tensor::cat(&[old_v, &extra_v], self.cat_dim)?.contiguous()?,
            ));
            self.capacity += grow_by;
        }

        let (storage_k, storage_v) = self.storage.as_mut().expect("storage initialized");
        storage_k.slice_set(k_new, self.cat_dim, self.cur_len)?;
        storage_v.slice_set(v_new, self.cat_dim, self.cur_len)?;
        self.cur_len = required;
        let k_all = storage_k.narrow(self.cat_dim, 0, self.cur_len)?;
        let v_all = storage_v.narrow(self.cat_dim, 0, self.cur_len)?;
        self.kv = Some((k_all.clone(), v_all.clone()));
        Ok((k_all, v_all))
    }

    /// Drop everything at sequence indices `>= len`. No-op if `len >= cur_len`.
    pub fn trim_to(&mut self, len: usize) -> Result<()> {
        if len >= self.cur_len {
            return Ok(());
        }
        if len == 0 {
            self.reset();
            return Ok(());
        }
        // `cur_len > 0` implies `kv.is_some()` by the invariant maintained in
        // `append` / `reset`.
        if self.kv.is_none() {
            return Err(candle_core::Error::Msg(
                "TrimmableKvCache::trim_to: cache empty but cur_len > 0".into(),
            ));
        }
        let (storage_k, storage_v) = self.storage.as_ref().ok_or_else(|| {
            candle_core::Error::Msg(
                "TrimmableKvCache::trim_to: storage empty but cur_len > 0".into(),
            )
        })?;
        let k = storage_k.narrow(self.cat_dim, 0, len)?;
        let v = storage_v.narrow(self.cat_dim, 0, len)?;
        self.kv = Some((k, v));
        self.cur_len = len;
        Ok(())
    }

    /// Gather the cache to keep only the supplied positions, in the supplied
    /// order — e.g. keep `[0..prefix_len)` (an accepted history) then append
    /// a selected subset of newer positions.
    ///
    /// Each index must be `< current_seq_len()`. Indices may be repeated,
    /// though typical callers pass distinct positions.
    pub fn keep_indices(&mut self, indices: &[u32]) -> Result<()> {
        if indices.is_empty() {
            self.reset();
            return Ok(());
        }
        for &i in indices {
            if (i as usize) >= self.cur_len {
                return Err(candle_core::Error::Msg(format!(
                    "TrimmableKvCache::keep_indices: index {} out of bounds (cur_len={})",
                    i, self.cur_len
                )));
            }
        }
        let Some((k, v)) = self.kv.as_ref() else {
            return Err(candle_core::Error::Msg(
                "TrimmableKvCache::keep_indices on empty cache".into(),
            ));
        };
        if indices.iter().enumerate().all(|(i, &x)| x as usize == i) {
            return self.trim_to(indices.len());
        }
        let device = k.device();
        // `Tensor::new(&[u32], device)` lands the slice directly via candle's
        // `NdArray for &[S]` impl — no `indices.to_vec()` allocation needed.
        let idx_t = Tensor::new(indices, device)?;
        let new_k = k.index_select(&idx_t, self.cat_dim)?.contiguous()?;
        let new_v = v.index_select(&idx_t, self.cat_dim)?.contiguous()?;
        self.cur_len = 0;
        self.kv = None;
        self.append(&new_k, &new_v).map(|_| ())
    }

    pub fn current_seq_len(&self) -> usize {
        self.cur_len
    }

    /// Ensure fixed-capacity storage exists without retaining a logical token.
    /// This is used by CUDA-graph decode paths whose device kernel writes at a
    /// runtime offset while the host only tracks the resulting logical length.
    pub fn initialize_storage(&mut self, template: &Tensor) -> Result<()> {
        self.initialize_storage_with_capacity(template, self.configured_capacity)
    }

    /// Ensure fixed-capacity storage of exactly `capacity` tokens exists.
    /// CUDA graphs capture the backing pointers and the physical head stride,
    /// so a larger existing allocation cannot be reused with a smaller
    /// captured `cache_len` value.
    pub fn initialize_storage_with_capacity(
        &mut self,
        template: &Tensor,
        capacity: usize,
    ) -> Result<()> {
        if capacity == 0 {
            return Err(candle_core::Error::Msg(
                "TrimmableKvCache capacity must be non-zero".into(),
            ));
        }
        let reusable = self.storage.as_ref().is_some_and(|(storage_k, storage_v)| {
            self.capacity == capacity
                && storage_k.dtype() == template.dtype()
                && storage_v.dtype() == template.dtype()
                && storage_k.device().same_device(template.device())
                && storage_v.device().same_device(template.device())
                && storage_k.dims().len() == template.dims().len()
                && storage_k
                    .dims()
                    .iter()
                    .zip(template.dims())
                    .enumerate()
                    .all(|(dim, (stored, new))| {
                        if dim == self.cat_dim {
                            *stored == capacity
                        } else {
                            stored == new
                        }
                    })
                && storage_v.dims() == storage_k.dims()
        });
        if !reusable {
            let mut shape = template.dims().to_vec();
            shape[self.cat_dim] = capacity;
            self.capacity = capacity;
            self.storage = Some((
                Tensor::zeros(shape.as_slice(), template.dtype(), template.device())?,
                Tensor::zeros(shape.as_slice(), template.dtype(), template.device())?,
            ));
            self.kv = None;
            self.cur_len = 0;
        }
        Ok(())
    }

    /// Return the fixed backing tensors used by dynamic CUDA-graph appends.
    pub fn storage(&self) -> Option<(Tensor, Tensor)> {
        self.storage.as_ref().map(|(k, v)| (k.clone(), v.clone()))
    }

    /// Update only the logical length after a device-side graph append.
    pub fn set_current_len(&mut self, len: usize) -> Result<()> {
        if len > self.capacity {
            return Err(candle_core::Error::Msg(format!(
                "TrimmableKvCache::set_current_len: {len} exceeds capacity {}",
                self.capacity
            )));
        }
        if len == 0 {
            self.kv = None;
            self.cur_len = 0;
            return Ok(());
        }
        let (storage_k, storage_v) = self.storage.as_ref().ok_or_else(|| {
            candle_core::Error::Msg(
                "TrimmableKvCache::set_current_len: storage is not initialized".into(),
            )
        })?;
        self.kv = Some((
            storage_k.narrow(self.cat_dim, 0, len)?,
            storage_v.narrow(self.cat_dim, 0, len)?,
        ));
        self.cur_len = len;
        Ok(())
    }

    pub fn max_seq_len(&self) -> usize {
        self.configured_capacity
    }

    /// Physical sequence capacity of the current backing storage.
    pub fn storage_capacity(&self) -> usize {
        self.capacity
    }

    pub fn reset(&mut self) {
        self.kv = None;
        self.cur_len = 0;
    }

    /// Borrow the current K cache, if any.
    pub fn k(&self) -> Option<&Tensor> {
        self.kv.as_ref().map(|(k, _)| k)
    }

    /// Borrow the current V cache, if any.
    pub fn v(&self) -> Option<&Tensor> {
        self.kv.as_ref().map(|(_, v)| v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use candle_core::{DType, Device};

    fn dev() -> Device {
        Device::Cpu
    }

    #[test]
    fn append_grows_and_returns_full_cache() -> Result<()> {
        let mut c = TrimmableKvCache::new(2, 64);
        let a = Tensor::zeros((1, 2, 3, 4), DType::F32, &dev())?;
        let b = Tensor::ones((1, 2, 5, 4), DType::F32, &dev())?;
        let (k1, _) = c.append(&a, &a)?;
        assert_eq!(k1.dims(), &[1, 2, 3, 4]);
        assert_eq!(c.current_seq_len(), 3);
        let (k2, _) = c.append(&b, &b)?;
        assert_eq!(k2.dims(), &[1, 2, 8, 4]);
        assert_eq!(c.current_seq_len(), 8);
        Ok(())
    }

    #[test]
    fn trim_to_shorter_drops_tail() -> Result<()> {
        let mut c = TrimmableKvCache::new(2, 64);
        let t = Tensor::zeros((1, 2, 6, 4), DType::F32, &dev())?;
        c.append(&t, &t)?;
        c.trim_to(4)?;
        assert_eq!(c.current_seq_len(), 4);
        assert_eq!(c.k().unwrap().dims(), &[1, 2, 4, 4]);
        Ok(())
    }

    #[test]
    fn trim_to_zero_resets() -> Result<()> {
        let mut c = TrimmableKvCache::new(2, 64);
        let t = Tensor::zeros((1, 2, 6, 4), DType::F32, &dev())?;
        c.append(&t, &t)?;
        c.trim_to(0)?;
        assert_eq!(c.current_seq_len(), 0);
        assert!(c.k().is_none());
        assert!(c.v().is_none());
        Ok(())
    }

    #[test]
    fn trim_to_longer_is_noop() -> Result<()> {
        let mut c = TrimmableKvCache::new(2, 64);
        let t = Tensor::zeros((1, 2, 3, 4), DType::F32, &dev())?;
        c.append(&t, &t)?;
        c.trim_to(10)?;
        assert_eq!(c.current_seq_len(), 3);
        Ok(())
    }

    #[test]
    fn reset_then_append_works() -> Result<()> {
        let mut c = TrimmableKvCache::new(2, 64);
        let t = Tensor::zeros((1, 2, 3, 4), DType::F32, &dev())?;
        c.append(&t, &t)?;
        c.reset();
        let s = Tensor::ones((1, 2, 2, 4), DType::F32, &dev())?;
        let (k, _) = c.append(&s, &s)?;
        assert_eq!(k.dims(), &[1, 2, 2, 4]);
        assert_eq!(c.current_seq_len(), 2);
        Ok(())
    }

    #[test]
    fn fixed_storage_uses_exact_requested_capacity() -> Result<()> {
        let mut c = TrimmableKvCache::new(2, 64);
        let template = Tensor::zeros((1, 2, 1, 4), DType::F32, &dev())?;
        c.initialize_storage_with_capacity(&template, 8)?;
        assert_eq!(c.storage_capacity(), 8);
        assert_eq!(c.storage().unwrap().0.dims(), &[1, 2, 8, 4]);

        c.set_current_len(4)?;
        c.initialize_storage_with_capacity(&template, 16)?;
        assert_eq!(c.storage_capacity(), 16);
        assert_eq!(c.current_seq_len(), 0);
        assert_eq!(c.storage().unwrap().0.dims(), &[1, 2, 16, 4]);
        Ok(())
    }

    #[test]
    fn fixed_storage_rejects_zero_capacity() {
        let mut c = TrimmableKvCache::new(2, 64);
        let template = Tensor::zeros((1, 2, 1, 4), DType::F32, &dev()).unwrap();
        assert!(c.initialize_storage_with_capacity(&template, 0).is_err());
    }

    #[test]
    fn incompatible_storage_starts_a_new_logical_cache() -> Result<()> {
        let mut c = TrimmableKvCache::new(2, 64);
        let old = Tensor::zeros((1, 1, 3, 4), DType::F32, &dev())?;
        c.append(&old, &old)?;

        // Changing the number of heads makes the retained storage
        // incompatible with the new sequence.
        let new = Tensor::ones((1, 2, 2, 4), DType::F32, &dev())?;
        let (k, v) = c.append(&new, &new)?;

        assert_eq!(c.current_seq_len(), 2);
        assert_eq!(k.dims(), &[1, 2, 2, 4]);
        assert_eq!(v.dims(), &[1, 2, 2, 4]);
        assert!(k.flatten_all()?.to_vec1::<f32>()?.iter().all(|&x| x == 1.0));
        Ok(())
    }

    #[test]
    fn trim_then_append_concats_correctly() -> Result<()> {
        let mut c = TrimmableKvCache::new(2, 64);
        let a = Tensor::zeros((1, 2, 6, 4), DType::F32, &dev())?;
        let b = Tensor::ones((1, 2, 3, 4), DType::F32, &dev())?;
        c.append(&a, &a)?;
        c.trim_to(4)?;
        let (k, _) = c.append(&b, &b)?;
        assert_eq!(k.dims(), &[1, 2, 7, 4]);
        assert_eq!(c.current_seq_len(), 7);
        Ok(())
    }

    #[test]
    fn empty_then_trim_is_noop() -> Result<()> {
        let mut c = TrimmableKvCache::new(2, 64);
        c.trim_to(5)?;
        assert_eq!(c.current_seq_len(), 0);
        Ok(())
    }

    /// Build a deterministic cache where K[..., t, 0] == t (so we can verify
    /// the gathered ordering after `keep_indices`).
    fn build_indexed_cache(len: usize) -> Result<TrimmableKvCache> {
        let mut c = TrimmableKvCache::new(2, 128);
        for t in 0..len {
            let k = Tensor::from_vec(vec![t as f32, 0.0, 0.0, 0.0], (1, 1, 1, 4), &dev())?;
            c.append(&k, &k)?;
        }
        Ok(c)
    }

    #[test]
    fn keep_indices_gathers_in_order() -> Result<()> {
        let mut c = build_indexed_cache(8)?;
        c.keep_indices(&[0, 1, 3, 5])?;
        assert_eq!(c.current_seq_len(), 4);
        let k = c.k().unwrap();
        let raw: Vec<f32> = k.flatten_all()?.to_vec1()?;
        assert_eq!(raw[0], 0.0);
        assert_eq!(raw[4], 1.0);
        assert_eq!(raw[8], 3.0);
        assert_eq!(raw[12], 5.0);
        Ok(())
    }

    #[test]
    fn keep_indices_prefix_uses_trim_fast_path() -> Result<()> {
        let mut c = build_indexed_cache(6)?;
        c.keep_indices(&[0, 1, 2])?;
        assert_eq!(c.current_seq_len(), 3);
        Ok(())
    }

    #[test]
    fn keep_indices_empty_resets() -> Result<()> {
        let mut c = build_indexed_cache(3)?;
        c.keep_indices(&[])?;
        assert_eq!(c.current_seq_len(), 0);
        assert!(c.k().is_none());
        Ok(())
    }

    #[test]
    fn keep_indices_out_of_bounds_errors() {
        let mut c = build_indexed_cache(3).unwrap();
        let err = c.keep_indices(&[0, 5]).unwrap_err().to_string();
        assert!(err.contains("out of bounds"), "unexpected error: {err}");
    }

    #[test]
    fn keep_indices_then_append_works() -> Result<()> {
        let mut c = build_indexed_cache(5)?;
        c.keep_indices(&[1, 3])?;
        let extra = Tensor::from_vec(vec![99.0f32, 0.0, 0.0, 0.0], (1, 1, 1, 4), &dev())?;
        c.append(&extra, &extra)?;
        assert_eq!(c.current_seq_len(), 3);
        let raw: Vec<f32> = c.k().unwrap().flatten_all()?.to_vec1()?;
        assert_eq!(raw[0], 1.0);
        assert_eq!(raw[4], 3.0);
        assert_eq!(raw[8], 99.0);
        Ok(())
    }
}
