//! Explicit measurement only: representative immutable Session and presence payloads.
//! Run with `cargo run --locked -p aw-agent-core --release --example publication_measure`.
//! This is not an end-to-end HTTP/authorization benchmark or a product migration.
use aw_agent_core::publication;
use std::{
    collections::BTreeMap,
    hint::black_box,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc, Barrier, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone)]
struct Presence {
    label: String,
    anchor: u32,
    updated_at: Instant,
}
enum Data {
    Session(Vec<u8>),
    Presence(BTreeMap<(String, u64, String, String), Presence>),
}
struct Accounting {
    live: AtomicUsize,
    peak: AtomicUsize,
    allocations: AtomicUsize,
}
impl Accounting {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            live: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            allocations: AtomicUsize::new(0),
        })
    }
    fn allocated(&self, bytes: usize) {
        self.allocations.fetch_add(1, Ordering::Relaxed);
        let live = self.live.fetch_add(bytes, Ordering::Relaxed) + bytes;
        self.peak.fetch_max(live, Ordering::Relaxed);
    }
}
struct Payload {
    data: Data,
    approximate_bytes: usize,
    accounting: Arc<Accounting>,
}
impl Payload {
    fn new(data: Data, accounting: Arc<Accounting>) -> Self {
        // Approximates payload and map metadata, not allocator headers or Arc overhead.
        let approximate_bytes = match &data {
            Data::Session(bytes) => bytes.len(),
            Data::Presence(rows) => rows
                .iter()
                .map(|(k, v)| 96 + k.0.len() + k.2.len() + k.3.len() + v.label.len())
                .sum(),
        };
        accounting.allocated(approximate_bytes);
        Self {
            data,
            approximate_bytes,
            accounting,
        }
    }
    fn advance(&mut self, iteration: u64) {
        match &mut self.data {
            Data::Session(bytes) => bytes[..8].copy_from_slice(&iteration.to_le_bytes()),
            Data::Presence(rows) => {
                if let Some((_, value)) = rows.iter_mut().next() {
                    value.anchor = iteration as u32;
                    value.updated_at = Instant::now();
                }
            }
        }
    }
    fn cleanup_presence(&mut self) {
        if let Data::Presence(rows) = &mut self.data {
            let now = Instant::now();
            rows.retain(|_, value| now.duration_since(value.updated_at) <= Duration::from_secs(30));
        }
    }
    fn inspect(&self) {
        match &self.data {
            Data::Session(bytes) => {
                black_box((
                    u64::from_le_bytes(bytes[..8].try_into().unwrap()),
                    bytes.len(),
                ));
            }
            Data::Presence(rows) => {
                // Same bounded, current-TTL, per-draft read-side filtering and owned
                // response projection as the real handler; authorization is separate.
                let now = Instant::now();
                let selected: Vec<_> = rows
                    .iter()
                    .filter(|((draft, generation, _, _), v)| {
                        draft == "draft"
                            && *generation == 1
                            && now.duration_since(v.updated_at) <= Duration::from_secs(30)
                    })
                    .map(|(key, value)| {
                        (
                            key.2.clone(),
                            key.3.clone(),
                            value.label.clone(),
                            value.anchor,
                        )
                    })
                    .collect();
                black_box(selected);
            }
        }
    }
}
impl Clone for Payload {
    fn clone(&self) -> Self {
        let data = match &self.data {
            Data::Session(bytes) => Data::Session(bytes.clone()),
            Data::Presence(rows) => Data::Presence(rows.clone()),
        };
        let cloned = Self::new(data, self.accounting.clone());
        debug_assert_eq!(cloned.approximate_bytes, self.approximate_bytes);
        cloned
    }
}
impl Drop for Payload {
    fn drop(&mut self) {
        self.accounting
            .live
            .fetch_sub(self.approximate_bytes, Ordering::Relaxed);
    }
}
#[derive(Clone, Copy)]
enum Shape {
    Session(usize),
    Presence(usize),
}
impl Shape {
    fn describe(self) -> String {
        match self {
            Self::Session(bytes) => format!("session_{bytes}_bytes"),
            Self::Presence(rows) => format!("presence_{rows}_rows"),
        }
    }
    fn create(self, accounting: Arc<Accounting>) -> Payload {
        let data = match self {
            Self::Session(bytes) => Data::Session(vec![b'x'; bytes]),
            Self::Presence(rows) => Data::Presence(
                (0..rows)
                    .map(|i| {
                        (
                            (
                                "draft".into(),
                                1,
                                format!("member_{}", i / 8),
                                format!("client_{i}"),
                            ),
                            Presence {
                                label: format!("member display name {i}"),
                                anchor: 0,
                                updated_at: Instant::now(),
                            },
                        )
                    })
                    .collect(),
            ),
        };
        Payload::new(data, accounting)
    }
}
fn percentiles(mut times: Vec<u128>) -> [u128; 2] {
    if times.is_empty() {
        return [0, 0];
    }
    times.sort_unstable();
    [50, 95].map(|p| times[(times.len() - 1) * p / 100])
}
fn cpu_ticks() -> Option<u64> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let (_, tail) = stat.rsplit_once(") ")?;
    let values: Vec<&str> = tail.split_whitespace().collect();
    Some(values.get(11)?.parse::<u64>().ok()? + values.get(12)?.parse::<u64>().ok()?)
}
fn run(
    shape: Shape,
    readers: usize,
    write_per_read: (usize, usize),
    published: bool,
    hold_old_snapshot: bool,
) {
    let read_each = match shape {
        Shape::Session(4_194_304) => 24,
        Shape::Session(_) => 96,
        Shape::Presence(_) => 384,
    };
    let total_reads = readers * read_each;
    let writes = total_reads * write_per_read.0 / write_per_read.1;
    let accounting = Accounting::new();
    let mut tasks = Vec::new();
    let barrier = Arc::new(Barrier::new(readers + 2));
    let (send, recv) = mpsc::channel();
    let mut cold_registration_ns = 0u128;
    // Keep the publisher alive even if the writer finishes before its readers;
    // the production helper deliberately fails closed after publisher shutdown.
    let mut publication_owner = None;
    let mut held_published = None;
    let mut held_baseline = None;
    if published {
        let (publisher, factory) = publication::channel(shape.create(accounting.clone()));
        let publisher = Arc::new(publisher);
        publication_owner = Some(publisher.clone());
        if hold_old_snapshot {
            held_published = Some(factory.reader().unwrap().snapshot().unwrap());
        }
        for _ in 0..readers {
            let start = Instant::now();
            let reader = factory.reader().unwrap();
            cold_registration_ns += start.elapsed().as_nanos();
            let barrier = barrier.clone();
            let sender = send.clone();
            tasks.push(thread::spawn(move || {
                let mut samples = Vec::new();
                barrier.wait();
                for _ in 0..read_each {
                    let start = Instant::now();
                    reader.snapshot().unwrap().value.inspect();
                    samples.push(start.elapsed().as_nanos());
                }
                sender.send((false, samples)).unwrap();
            }));
        }
        let barrier = barrier.clone();
        let sender = send.clone();
        tasks.push(thread::spawn(move || {
            let mut samples = Vec::new();
            barrier.wait();
            for i in 1..=writes {
                let start = Instant::now();
                publisher
                    .update(|old| {
                        let mut next = old.clone();
                        next.advance(i as u64);
                        Ok((next, ()))
                    })
                    .unwrap();
                samples.push(start.elapsed().as_nanos());
                if i % 16 == 0 {
                    thread::yield_now();
                }
            }
            sender.send((true, samples)).unwrap();
        }));
    } else {
        let payload = Arc::new(Mutex::new(shape.create(accounting.clone())));
        if hold_old_snapshot {
            held_baseline = Some(payload.lock().unwrap().clone());
        }
        for _ in 0..readers {
            let registration = Instant::now();
            let payload = payload.clone();
            cold_registration_ns += registration.elapsed().as_nanos();
            let barrier = barrier.clone();
            let sender = send.clone();
            tasks.push(thread::spawn(move || {
                let mut samples = Vec::new();
                barrier.wait();
                for _ in 0..read_each {
                    let start = Instant::now();
                    let mut locked = payload.lock().unwrap();
                    match &locked.data {
                        Data::Session(_) => {
                            let owned = locked.clone();
                            drop(locked);
                            owned.inspect();
                        }
                        Data::Presence(_) => {
                            // The live handler retains/filter-projects under its
                            // Mutex; it does NOT clone the entire presence map.
                            locked.cleanup_presence();
                            locked.inspect();
                        }
                    }
                    samples.push(start.elapsed().as_nanos());
                }
                sender.send((false, samples)).unwrap();
            }));
        }
        let barrier = barrier.clone();
        let sender = send.clone();
        tasks.push(thread::spawn(move || {
            let mut samples = Vec::new();
            barrier.wait();
            for i in 1..=writes {
                let start = Instant::now();
                payload.lock().unwrap().advance(i as u64);
                samples.push(start.elapsed().as_nanos());
                if i % 16 == 0 {
                    thread::yield_now();
                }
            }
            sender.send((true, samples)).unwrap();
        }));
    }
    drop(send);
    let cpu_start = cpu_ticks();
    let start = Instant::now();
    barrier.wait();
    for task in tasks {
        task.join().unwrap();
    }
    drop(publication_owner);
    let wall_ns = start.elapsed().as_nanos();
    let cpu_delta = cpu_ticks().zip(cpu_start).map(|(a, b)| a.saturating_sub(b));
    let mut read_samples = Vec::new();
    let mut write_samples = Vec::new();
    for (write, samples) in recv {
        if write {
            write_samples.extend(samples);
        } else {
            read_samples.extend(samples);
        }
    }
    println!(
        "{}",
        serde_json::json!({
            "shape": shape.describe(), "strategy": if published { "left_right" } else { "mutex" },
            "readers": readers, "reads": total_reads, "writes": writes,
            "write_read_ratio": format!("{}:{}", write_per_read.0, write_per_read.1),
            "wall_ns": wall_ns, "cpu_ticks_linux": cpu_delta,
            "read_p50_p95_ns": percentiles(read_samples),
            "write_p50_p95_ns": percentiles(write_samples),
            "cold_reader_registration_ns": cold_registration_ns,
            "held_old_snapshot": hold_old_snapshot,
            "payload_allocations": accounting.allocations.load(Ordering::Relaxed),
            "peak_estimated_payload_bytes": accounting.peak.load(Ordering::Relaxed),
            "note": "isolated representative payload benchmark; does not include HTTP JSON encoding, Store/grant checks or SSE high-water coordination"
        })
    );
    // Force both historical snapshots to outlive the entire workload/report.
    drop(held_published);
    drop(held_baseline);
}
fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let quick = args.iter().any(|arg| arg == "--quick");
    let held = args.iter().any(|arg| arg == "--held");
    let shapes = if quick {
        vec![
            Shape::Session(32_768),
            Shape::Presence(8),
            Shape::Presence(128),
        ]
    } else {
        vec![
            Shape::Session(32_768),
            Shape::Session(262_144),
            Shape::Session(4_194_304),
            Shape::Presence(8),
            Shape::Presence(128),
        ]
    };
    for shape in shapes {
        for readers in if quick { vec![1, 4] } else { vec![1, 4, 8] } {
            for ratio in [(1, 4), (1, 16), (4, 1)] {
                for published in [false, true] {
                    run(shape, readers, ratio, published, held);
                }
            }
        }
    }
}
