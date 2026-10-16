use super::*;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

struct Reader {
    stamp: Stamp,
    bytes: std::io::Cursor<Vec<u8>>,
    reads: Arc<AtomicUsize>,
    on_read: Box<dyn FnMut() + Send>,
    changed: bool,
}
impl Reader {
    fn new(content: &[u8]) -> Self {
        Self {
            stamp: Stamp {
                dev: 1,
                inode: 2,
                size: content.len() as u64,
                mtime: (3, 4),
                ctime: (5, 6),
            },
            bytes: std::io::Cursor::new(content.to_vec()),
            reads: Arc::new(AtomicUsize::new(0)),
            on_read: Box::new(|| {}),
            changed: false,
        }
    }
}
impl Read for Reader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        assert_eq!(buffer.len(), BUFFER_SIZE);
        self.reads.fetch_add(1, Ordering::SeqCst);
        (self.on_read)();
        self.bytes.read(buffer)
    }
}
impl ImageReader for Reader {
    fn stamp(&self) -> io::Result<Stamp> {
        let mut stamp = self.stamp;
        if self.changed && self.reads.load(Ordering::SeqCst) > 0 {
            stamp.ctime.1 += 1;
        }
        Ok(stamp)
    }
}
fn control() -> Control {
    Control::new(Cancel::default(), Arc::new(|| Duration::ZERO))
}

#[test]
fn digest_cache_hit_does_not_read_image_again() {
    let cache = Cache::default();
    let mut first = Reader::new(b"abc");
    let digest = cache.digest(&mut first, &control()).unwrap();
    assert_eq!(
        digest,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let mut second = Reader::new(b"abc");
    assert_eq!(cache.digest(&mut second, &control()).unwrap(), digest);
    assert_eq!(second.reads.load(Ordering::SeqCst), 0);
}

#[test]
fn image_replacement_each_stamp_field_invalidates_cache() {
    let cache = Cache::default();
    let old = cache.digest(&mut Reader::new(b"old"), &control()).unwrap();
    for field in 0..7 {
        let mut updated = Reader::new(b"new");
        match field {
            0 => updated.stamp.dev += 1,
            1 => updated.stamp.inode += 1,
            2 => {
                updated = Reader::new(b"longer");
            }
            3 => updated.stamp.mtime.0 += 1,
            4 => updated.stamp.mtime.1 += 1,
            5 => updated.stamp.ctime.0 += 1,
            _ => updated.stamp.ctime.1 += 1,
        }
        let digest = cache.digest(&mut updated, &control()).unwrap();
        assert_ne!(digest, old);
        assert!(updated.reads.load(Ordering::SeqCst) > 0);
        let make_identity = |sha256| super::super::AppIdentity {
            exe_path: "/fixture".into(),
            dev: 1,
            inode: 2,
            sha256,
            pid: 1,
            start_time: 1,
            desktop_id: None,
        };
        assert!(super::super::needs_reauth(
            &make_identity(old.clone()),
            &make_identity(digest)
        ));
    }
}

#[test]
fn long_hash_cancellation_stops_reads_and_does_not_cache_partial_digest() {
    let cache = Cache::default();
    let cancel = Cancel::default();
    let signal = cancel.clone();
    let mut reader = Reader::new(&vec![1; BUFFER_SIZE * 3]);
    reader.on_read = Box::new(move || signal.cancel());
    let err = cache
        .digest(
            &mut reader,
            &Control::new(cancel, Arc::new(|| Duration::ZERO)),
        )
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
    assert_eq!(reader.reads.load(Ordering::SeqCst), 1);
    let mut retry = Reader::new(&vec![1; BUFFER_SIZE * 3]);
    cache.digest(&mut retry, &control()).unwrap();
    assert!(retry.reads.load(Ordering::SeqCst) > 1);
}

#[test]
fn huge_image_virtual_deadline_times_out_without_caching_partial_digest() {
    let cache = Cache::default();
    let time = Arc::new(AtomicU64::new(0));
    let clock = time.clone();
    let mut reader = Reader::new(b"small physical fixture");
    reader.stamp.size = MAX_IMAGE_SIZE;
    reader.on_read = Box::new(move || {
        time.store(HASH_TIMEOUT.as_secs(), Ordering::SeqCst);
    });
    let err = cache
        .digest(
            &mut reader,
            &Control::new(
                Cancel::default(),
                Arc::new(move || Duration::from_secs(clock.load(Ordering::SeqCst))),
            ),
        )
        .unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    assert_eq!(reader.reads.load(Ordering::SeqCst), 1);
    // Same key must be read again, never accepted as the truncated fixture's digest.
    let mut retry = Reader::new(b"small physical fixture");
    retry.stamp.size = MAX_IMAGE_SIZE;
    assert_eq!(
        cache.digest(&mut retry, &control()).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    assert!(retry.reads.load(Ordering::SeqCst) > 0);
}

#[test]
fn image_mutated_during_hash_or_short_read_is_never_cached() {
    for changed in [true, false] {
        let cache = Cache::default();
        let mut reader = Reader::new(b"abc");
        reader.changed = changed;
        if !changed {
            reader.stamp.size += 1;
        }
        assert_eq!(
            cache.digest(&mut reader, &control()).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        reader.bytes.set_position(0);
        reader.reads.store(0, Ordering::SeqCst);
        assert!(cache.digest(&mut reader, &control()).is_err());
        assert!(reader.reads.load(Ordering::SeqCst) > 0);
    }
}

#[test]
fn concurrent_same_image_single_flight_reads_only_once() {
    let cache = Arc::new(Cache::default());
    let reads = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(std::sync::Barrier::new(3));
    let jobs: Vec<_> = (0..2)
        .map(|_| {
            let (cache, reads, start) = (cache.clone(), reads.clone(), start.clone());
            std::thread::spawn(move || {
                let mut reader = Reader::new(b"abc");
                reader.reads = reads;
                start.wait();
                cache.digest(&mut reader, &control()).unwrap()
            })
        })
        .collect();
    start.wait();
    let digests: Vec<_> = jobs.into_iter().map(|j| j.join().unwrap()).collect();
    assert_eq!(digests[0], digests[1]);
    assert_eq!(reads.load(Ordering::SeqCst), 2); // one chunk and EOF
}

#[test]
fn cache_hit_virtual_deadline_or_cancellation_never_returns_trust() {
    let cache = Cache::default();
    cache.digest(&mut Reader::new(b"abc"), &control()).unwrap();
    let cancel = Cancel::default();
    let time = Arc::new(AtomicU64::new(0));
    let clock = time.clone();
    let control = Control::new(
        cancel.clone(),
        Arc::new(move || Duration::from_secs(clock.load(Ordering::SeqCst))),
    );
    time.store(HASH_TIMEOUT.as_secs(), Ordering::SeqCst);
    assert_eq!(
        cache
            .digest(&mut Reader::new(b"abc"), &control)
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut
    );
    cancel.cancel();
    assert_eq!(
        cache
            .digest(&mut Reader::new(b"abc"), &control)
            .unwrap_err()
            .kind(),
        io::ErrorKind::Interrupted
    );
}

#[test]
fn oversized_image_rejected_before_any_read() {
    let mut reader = Reader::new(b"abc");
    reader.stamp.size = MAX_IMAGE_SIZE + 1;
    assert_eq!(
        Cache::default()
            .digest(&mut reader, &control())
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(reader.reads.load(Ordering::SeqCst), 0);
}

#[test]
fn native_file_content_rewrite_and_inode_replacement_rehash_and_reauthorize() {
    use std::fs::{File, FileTimes};
    use std::time::UNIX_EPOCH;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image");
    let cache = Cache::default();
    let identity = |file: &mut File| {
        let stamp = file.stamp().unwrap();
        super::super::AppIdentity {
            exe_path: path.clone(),
            dev: stamp.dev,
            inode: stamp.inode,
            sha256: cache.digest(file, &control()).unwrap(),
            pid: 1,
            start_time: 1,
            desktop_id: None,
        }
    };
    std::fs::write(&path, b"old").unwrap();
    let before = identity(&mut File::open(&path).unwrap());
    // Force a known different mtime without sleeps or dependence on FS clock granularity.
    std::fs::write(&path, b"new").unwrap();
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(123)))
        .unwrap();
    let after = identity(&mut File::open(&path).unwrap());
    assert_eq!(before.inode, after.inode);
    assert_ne!(before.sha256, after.sha256);
    assert!(super::super::needs_reauth(&before, &after));
    let replacement = directory.path().join("replacement");
    std::fs::write(&replacement, b"new").unwrap();
    std::fs::rename(&replacement, &path).unwrap();
    let replaced = identity(&mut File::open(&path).unwrap());
    assert_ne!(after.inode, replaced.inode);
    assert!(super::super::needs_reauth(&after, &replaced));
}

#[test]
fn cold_hash_past_native_deadline_within_hash_budget_succeeds() {
    let time = Arc::new(AtomicU64::new(0));
    let clock = time.clone();
    let control = Control::new(
        Cancel::default(),
        Arc::new(move || Duration::from_secs(clock.load(Ordering::SeqCst))),
    );
    let mut reader = Reader::new(b"abc");
    reader.on_read = Box::new(move || {
        time.store(30, Ordering::SeqCst);
    });
    assert_eq!(
        Cache::default().digest(&mut reader, &control).unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
