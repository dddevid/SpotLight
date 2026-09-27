use crate::app::Track;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueSource {
    Playlist,
    #[allow(dead_code)]
    Search,
    User,
    Recommended,
}

#[derive(Clone, Debug)]
pub struct QueueItem {
    pub track: Track,
    pub source: QueueSource,
}

#[derive(Clone, Default)]
pub struct PlaybackQueue {
    items: Vec<QueueItem>,
    current_index: Option<usize>,
}

impl PlaybackQueue {
    pub fn items(&self) -> &[QueueItem] {
        &self.items
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn current(&self) -> Option<&QueueItem> {
        self.current_index.and_then(|index| self.items.get(index))
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.current_index = None;
    }

    pub fn push(&mut self, track: Track, source: QueueSource) -> bool {
        if self.items.iter().any(|item| item.track.uri == track.uri) {
            return false;
        }
        self.items.push(QueueItem { track, source });
        true
    }

    pub fn extend_unique<I>(&mut self, tracks: I, source: QueueSource) -> usize
    where
        I: IntoIterator<Item = Track>,
    {
        tracks
            .into_iter()
            .filter(|track| self.push(track.clone(), source.clone()))
            .count()
    }

    pub fn remove(&mut self, index: usize) -> Option<QueueItem> {
        if index >= self.items.len() {
            return None;
        }

        let removed = self.items.remove(index);
        if let Some(current) = self.current_index {
            self.current_index = if index < current {
                Some(current - 1)
            } else if index == current {
                None
            } else {
                Some(current)
            };
        }
        Some(removed)
    }

    pub fn set_current(&mut self, index: usize) -> Option<&QueueItem> {
        if index >= self.items.len() {
            return None;
        }
        self.current_index = Some(index);
        self.items.get(index)
    }

    pub fn next_index(&self) -> Option<usize> {
        match self.current_index {
            Some(index) if index + 1 < self.items.len() => Some(index + 1),
            None if !self.items.is_empty() => Some(0),
            _ => None,
        }
    }

    pub fn smart_shuffle(&mut self, seed: u64) {
        if self.items.len() < 2 {
            return;
        }

        let current_uri = self.current().map(|item| item.track.uri.clone());
        let start = self.current_index.map(|index| index + 1).unwrap_or(0);
        let mut state = seed.max(1);
        for index in (start + 1..self.items.len()).rev() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let swap_index = start + (state as usize % (index - start + 1));
            self.items.swap(index, swap_index);
        }

        if let Some(current_uri) = current_uri {
            if let Some(index) = self.items.iter().position(|item| item.track.uri == current_uri) {
                if index != self.current_index.unwrap_or(index) {
                    self.items.swap(index, self.current_index.unwrap_or(index));
                }
            }
        }

        for index in start + 1..self.items.len() {
            let same_artist = self.items[index - 1].track.artist == self.items[index].track.artist;
            if same_artist {
                if let Some(other_index) = ((index + 1)..self.items.len())
                    .find(|candidate| self.items[*candidate].track.artist != self.items[index - 1].track.artist)
                {
                    self.items.swap(index, other_index);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(uri: &str, artist: &str) -> Track {
        Track {
            uri: uri.to_string(),
            artist: artist.to_string(),
            ..Track::default()
        }
    }

    #[test]
    fn queue_deduplicates_by_uri() {
        let mut queue = PlaybackQueue::default();
        assert!(queue.push(track("spotify:track:1", "A"), QueueSource::Playlist));
        assert!(!queue.push(track("spotify:track:1", "A"), QueueSource::Recommended));
        assert_eq!(queue.items().len(), 1);
    }

    #[test]
    fn smart_shuffle_keeps_current_track_in_place() {
        let mut queue = PlaybackQueue::default();
        queue.extend_unique(
            [track("1", "A"), track("2", "B"), track("3", "C")],
            QueueSource::Playlist,
        );
        queue.set_current(0);
        queue.smart_shuffle(42);
        assert_eq!(queue.current().map(|item| item.track.uri.as_str()), Some("1"));
    }

    #[test]
    fn next_index_stops_at_end() {
        let mut queue = PlaybackQueue::default();
        queue.extend_unique([track("1", "A"), track("2", "B")], QueueSource::Playlist);
        assert_eq!(queue.next_index(), Some(0));
        queue.set_current(1);
        assert_eq!(queue.next_index(), None);
    }
}