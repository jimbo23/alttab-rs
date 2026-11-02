use crate::ItemKey;
use std::collections::BTreeMap;
use std::fmt;

pub const DEFAULT_THUMBNAIL_BUDGET: usize = 32 * 1024 * 1024;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PixelSize {
    pub width: u32,
    pub height: u32,
}
impl PixelSize {
    pub fn bgra_bytes(self) -> Option<usize> {
        (self.width as usize)
            .checked_mul(4)?
            .checked_mul(self.height as usize)
    }
    pub fn bytes_with_stride(self, bytes_per_row: usize) -> Option<usize> {
        if bytes_per_row < (self.width as usize).checked_mul(4)? {
            return None;
        }
        bytes_per_row.checked_mul(self.height as usize)
    }
}
pub fn downscale_to_budget(source: PixelSize, max_bytes: usize) -> Option<PixelSize> {
    if source.width == 0 || source.height == 0 || max_bytes < 4 {
        return None;
    }
    let source_bytes = source.bgra_bytes()?;
    if source_bytes <= max_bytes {
        return Some(source);
    }
    let scale = (max_bytes as f64 / source_bytes as f64).sqrt();
    let mut width = ((source.width as f64 * scale).floor() as u32).max(1);
    let mut height = ((source.height as f64 * scale).floor() as u32).max(1);
    // A one-pixel minor axis can exceed the continuous fit for extreme aspects.
    let max_pixels = max_bytes / 4;
    if width as usize * height as usize > max_pixels {
        if width >= height {
            width = (max_pixels / height as usize) as u32;
        } else {
            height = (max_pixels / width as usize) as u32;
        }
    }
    if width == 0 || height == 0 {
        return None;
    }
    Some(PixelSize { width, height })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheError {
    Hidden,
    Oversized,
    InvalidSize,
    Stale,
}
impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Hidden => "capture cache is hidden",
            Self::Oversized => "image exceeds cache budget",
            Self::InvalidSize => "invalid pixel size or byte charge",
            Self::Stale => "stale capture reservation",
        })
    }
}
impl std::error::Error for CacheError {}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reservation {
    pub token: u64,
    pub generation: u64,
    pub evicted: Vec<ItemKey>,
}
#[derive(Clone, Debug)]
struct Entry {
    variant: PixelSize,
    bytes: usize,
    touched: u64,
    token: u64,
    ready: bool,
}
/// Owns charges, not native image handles. Every returned eviction must drop its adapter payload.
#[derive(Clone, Debug)]
pub struct ThumbnailCache {
    entries: BTreeMap<ItemKey, Entry>,
    budget: usize,
    charged: usize,
    generation: u64,
    serial: u64,
    epoch: u64,
    active: bool,
    evict_on_hide: bool,
}
impl Default for ThumbnailCache {
    fn default() -> Self {
        Self::new(DEFAULT_THUMBNAIL_BUDGET)
    }
}
impl ThumbnailCache {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            budget: budget_bytes,
            charged: 0,
            generation: 0,
            serial: 0,
            epoch: 0,
            active: false,
            evict_on_hide: true,
        }
    }
    pub fn begin_session(&mut self) -> u64 {
        if !self.active {
            self.generation = self.generation.wrapping_add(1);
            self.active = true;
        }
        self.generation
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn budget(&self) -> usize {
        self.budget
    }
    pub fn set_evict_on_hide(&mut self, enabled: bool) {
        self.evict_on_hide = enabled;
    }
    pub fn charged_bytes(&self) -> usize {
        self.charged
    }
    pub fn ready_bytes(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| entry.ready)
            .map(|entry| entry.bytes)
            .sum()
    }
    pub fn contains(&self, key: ItemKey) -> bool {
        self.entries.get(&key).is_some_and(|entry| entry.ready)
    }
    pub fn variant(&self, key: ItemKey) -> Option<PixelSize> {
        self.entries
            .get(&key)
            .filter(|entry| entry.ready)
            .map(|entry| entry.variant)
    }
    pub fn reserve(
        &mut self,
        key: ItemKey,
        variant: PixelSize,
        bytes: usize,
    ) -> Result<Reservation, CacheError> {
        if !self.active {
            return Err(CacheError::Hidden);
        }
        if variant.width == 0
            || variant.height == 0
            || variant.bgra_bytes().is_none_or(|minimum| minimum > bytes)
        {
            return Err(CacheError::InvalidSize);
        }
        if bytes > self.budget {
            return Err(CacheError::Oversized);
        }
        let mut evicted = Vec::new();
        if self.remove(key) {
            evicted.push(key);
        }
        self.evict_until(self.budget - bytes, &mut evicted);
        self.serial = self.serial.wrapping_add(1);
        self.epoch = self.epoch.wrapping_add(1);
        self.entries.insert(
            key,
            Entry {
                variant,
                bytes,
                touched: self.epoch,
                token: self.serial,
                ready: false,
            },
        );
        self.charged += bytes;
        Ok(Reservation {
            token: self.serial,
            generation: self.generation,
            evicted,
        })
    }
    pub fn complete(
        &mut self,
        token: u64,
        generation: u64,
        actual_bytes: usize,
    ) -> Result<Vec<ItemKey>, CacheError> {
        let key = self.pending_key(token, generation)?;
        let entry = self.entries.get(&key).expect("pending entry");
        if entry
            .variant
            .bgra_bytes()
            .is_none_or(|minimum| minimum > actual_bytes)
        {
            self.remove(key);
            return Err(CacheError::InvalidSize);
        }
        if actual_bytes > self.budget {
            self.remove(key);
            return Err(CacheError::Oversized);
        }
        let mut entry = self.entries.remove(&key).expect("pending entry");
        self.charged -= entry.bytes;
        let mut evicted = Vec::new();
        self.evict_until(self.budget - actual_bytes, &mut evicted);
        entry.bytes = actual_bytes;
        entry.ready = true;
        self.charged += actual_bytes;
        self.entries.insert(key, entry);
        Ok(evicted)
    }
    pub fn fail(&mut self, token: u64, generation: u64) -> bool {
        match self.pending_key(token, generation) {
            Ok(key) => self.remove(key),
            Err(_) => false,
        }
    }
    fn pending_key(&self, token: u64, generation: u64) -> Result<ItemKey, CacheError> {
        if !self.active || generation != self.generation {
            return Err(CacheError::Stale);
        }
        self.entries
            .iter()
            .find(|(_, entry)| entry.token == token && !entry.ready)
            .map(|(key, _)| *key)
            .ok_or(CacheError::Stale)
    }
    pub fn touch(&mut self, key: ItemKey) {
        if let Some(entry) = self.entries.get_mut(&key) {
            self.epoch = self.epoch.wrapping_add(1);
            entry.touched = self.epoch;
        }
    }
    pub fn remove(&mut self, key: ItemKey) -> bool {
        if let Some(entry) = self.entries.remove(&key) {
            self.charged -= entry.bytes;
            true
        } else {
            false
        }
    }
    pub fn hide(&mut self) -> Vec<ItemKey> {
        self.active = false;
        self.generation = self.generation.wrapping_add(1);
        let evicted: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, entry)| self.evict_on_hide || !entry.ready)
            .map(|(key, _)| *key)
            .collect();
        for key in &evicted {
            self.remove(*key);
        }
        evicted
    }
    pub fn set_budget(&mut self, bytes: usize) -> Vec<ItemKey> {
        self.budget = bytes;
        let mut evicted = Vec::new();
        self.evict_until(bytes, &mut evicted);
        evicted
    }
    fn evict_until(&mut self, limit: usize, evicted: &mut Vec<ItemKey>) {
        while self.charged > limit {
            let Some(key) = self
                .entries
                .iter()
                .min_by_key(|(key, entry)| (entry.touched, **key))
                .map(|(key, _)| *key)
            else {
                break;
            };
            self.remove(key);
            evicted.push(key);
        }
    }
}

/// Demand-driven cadence; the caller supplies monotonic milliseconds, never a polling loop.
#[derive(Clone, Debug)]
pub struct CaptureCadence {
    last_capture: BTreeMap<ItemKey, u64>,
    selected_interval_ms: u64,
    other_interval_ms: u64,
}
impl Default for CaptureCadence {
    fn default() -> Self {
        Self::from_config(&crate::CaptureConfig::default())
    }
}
impl CaptureCadence {
    pub fn from_config(config: &crate::CaptureConfig) -> Self {
        let selected_hz = u64::from(config.selected_max_hz.clamp(1, 5));
        Self {
            last_capture: BTreeMap::new(),
            selected_interval_ms: 1000_u64.div_ceil(selected_hz),
            other_interval_ms: 1000,
        }
    }
    pub fn admit(
        &mut self,
        key: ItemKey,
        now_ms: u64,
        selected: bool,
        visible: bool,
        style: crate::Style,
        permitted: bool,
    ) -> bool {
        if !visible || style != crate::Style::Thumbnails || !permitted {
            return false;
        }
        let interval = if selected {
            self.selected_interval_ms
        } else {
            self.other_interval_ms
        };
        if self
            .last_capture
            .get(&key)
            .is_some_and(|last| now_ms.saturating_sub(*last) < interval)
        {
            return false;
        }
        self.last_capture.insert(key, now_ms);
        true
    }
    pub fn hide(&mut self) {
        self.last_capture.clear();
    }
}
