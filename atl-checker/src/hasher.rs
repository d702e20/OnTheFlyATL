#[cfg(not(target_os = "windows"))]
use fasthash::FastHasher;
use std::marker::PhantomData;

//#[cfg(not(target_os = "windows"))]
//pub type EdgHasher = FixedState<fasthash::xx::Hash64>;

//#[cfg(target_os = "windows")]
pub type EdgHasher = std::collections::hash_map::RandomState;

#[cfg(not(target_os = "windows"))]
#[derive(Clone)]
pub struct FixedState<H: fasthash::FastHash> {
    phantom: PhantomData<H>,
}

#[cfg(not(target_os = "windows"))]
impl<H: fasthash::FastHash> core::hash::BuildHasher for FixedState<H> {
    type Hasher = H::FastHasher;

    #[inline(always)]
    fn build_hasher(&self) -> Self::Hasher {
        Self::Hasher::new()
    }
}

#[cfg(not(target_os = "windows"))]
impl<H: fasthash::FastHash> Default for FixedState<H> {
    fn default() -> Self {
        FixedState {
            phantom: PhantomData,
        }
    }
}
