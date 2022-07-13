use std::{
    borrow::{Borrow, BorrowMut},
    cell::{Ref, RefCell},
    cmp::min,
    mem::replace,
    ops::{Deref, DerefMut},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};

use config::GLOBAL_CONFIG;
use crossbeam::channel::internal::SelectHandle;
use lazy_static::lazy_static;
use logger::{debug, info, warn};
use models::{FieldId, Timestamp, ValueType};
use parking_lot::Mutex;
use tokio::sync::{mpsc::UnboundedSender, RwLock};
use utils::BloomFilter;

use crate::{
    compaction::FlushReq,
    direct_io::{File, FileCursor},
    error::{Error, Result},
    file_manager::get_file_manager,
    file_utils,
    kv_option::TseriesFamOpt,
    memcache::{DataType, MemCache},
    summary::{CompactMeta, VersionEdit},
    tsm::{BlockReader, ColumnReader, Index, IndexReader},
    ColumnFileId, TseriesFamilyId, VersionId,
};

lazy_static! {
    pub static ref FLUSH_REQ: Arc<Mutex<Vec<FlushReq>>> = Arc::new(Mutex::new(vec![]));
}

#[derive(Default, Debug)]
pub struct TimeRange {
    pub max_ts: i64,
    pub min_ts: i64,
}

impl TimeRange {
    pub fn new(max_ts: i64, min_ts: i64) -> Self {
        Self { max_ts, min_ts }
    }

    pub fn overlaps(&self, range: &TimeRange) -> bool {
        !(self.min_ts > range.max_ts || self.max_ts < range.min_ts)
    }
}

#[derive(Debug)]
pub struct ColumnFile {
    file_id: ColumnFileId,
    being_compact: AtomicBool,
    deleted: AtomicBool,
    range: TimeRange, // file time range
    size: u64,        // file size
    field_id_bloom_filter: BloomFilter,
    is_delta: bool,
}

impl ColumnFile {
    pub fn file_id(&self) -> ColumnFileId {
        self.file_id
    }
    pub fn size(&self) -> u64 {
        self.size
    }
    pub fn range(&self) -> &TimeRange {
        &self.range
    }

    pub fn file(&self, tsf_opt: Arc<TseriesFamOpt>) -> Result<File> {
        let p = file_utils::make_tsm_file_name(&tsf_opt.tsm_dir, self.file_id);
        get_file_manager().open_file(p)
    }

    pub fn tombstone_file(&self, tsf_opt: Arc<TseriesFamOpt>) -> Result<File> {
        let p = file_utils::make_tsm_tombstone_file_name(&tsf_opt.tsm_dir, self.file_id);
        get_file_manager().open_file(p)
    }

    pub fn file_reader(&self, tf_id: u32) -> Result<(FileCursor, u64), Error> {
        let fs = get_file_manager();
        let ts_cf = TseriesFamOpt::default();
        let fs = if self.is_delta {
            let p = format!("/_{:06}.delta", self.file_id());
            fs.open_file(ts_cf.delta_dir + tf_id.to_string().as_str() + p.as_str())
        } else {
            let p = format!("/_{:06}.tsm", self.file_id());
            fs.open_file(ts_cf.tsm_dir + tf_id.to_string().as_str() + p.as_str())
        };
        match fs {
            Ok(v) => {
                let len = v.len();
                Ok((v.into_cursor(), len))
            },
            Err(err) => Err(err),
        }
    }

    pub fn overlap(&self, time_range: &TimeRange) -> bool {
        self.range.overlaps(time_range)
    }
}

impl ColumnFile {
    pub fn is_deleted(&self) -> bool {
        self.deleted.load(Ordering::Acquire)
    }

    pub fn mark_removed(&self) {
        self.deleted.store(true, Ordering::Release);
    }

    pub fn mark_compaction(&self) {
        self.being_compact.store(true, Ordering::Release);
    }

    pub fn is_pending_compaction(&self) -> bool {
        self.being_compact.load(Ordering::Acquire)
    }

    pub fn contains_field_id(&self, field_id: FieldId) -> bool {
        self.field_id_bloom_filter.contains(&field_id.to_be_bytes())
    }
}

#[derive(Default, Debug)]
pub struct LevelInfo {
    pub files: Vec<Arc<ColumnFile>>,
    pub tsf_opt: Arc<TseriesFamOpt>,
    pub level: u32,
    pub cur_size: u64,
    pub max_size: u64,
    pub ts_range: TimeRange,
}

impl LevelInfo {
    pub fn init(level: u32) -> Self {
        Self { files: Vec::new(),
               tsf_opt: Arc::new(TseriesFamOpt::default()),
               level,
               cur_size: 0,
               max_size: 0,
               ts_range: TimeRange { max_ts: 0, min_ts: 0 } }
    }
    pub fn apply(&mut self, delta: &CompactMeta) {
        self.files.push(Arc::new(ColumnFile { file_id: delta.file_id,
                                              being_compact: AtomicBool::new(false),
                                              deleted: AtomicBool::new(false),
                                              range: TimeRange::new(delta.ts_max,
                                                                    delta.ts_min),
                                              size: delta.file_size,
                                              field_id_bloom_filter: BloomFilter::new(512),
                                              is_delta: delta.is_delta }));
        self.cur_size += delta.file_size;
        if self.ts_range.max_ts < delta.ts_max {
            self.ts_range.max_ts = delta.ts_max;
        }
        if self.ts_range.min_ts > delta.ts_max {
            self.ts_range.min_ts = delta.ts_min;
        }
    }
    pub fn read_columnfile(&self, tf_id: u32, field_id: FieldId, time_range: &TimeRange) {
        for file in self.files.iter() {
            if file.is_deleted() || !file.overlap(time_range) {
                continue;
            }
            let file = file.file(self.tsf_opt.clone()).unwrap();
            let file = Arc::new(file);

            let index = IndexReader::open(file.clone()).unwrap();
            for idx in index.iter_opt(field_id) {
                for blk in idx.iter() {
                    if blk.min_ts() < time_range.max_ts && blk.max_ts() > time_range.min_ts {
                        let mut cr = ColumnReader::new(file.clone(),
                                                       idx.iter_opt(time_range.min_ts,
                                                                    time_range.max_ts));
                        while let Some(blk_ret) = cr.next() {
                            if let Ok(blk) = blk_ret {
                                println!("{:?}", &blk);
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn level(&self) -> u32 {
        self.level
    }
}

#[derive(Default)]
pub struct Version {
    pub id: VersionId,
    pub last_seq: u64,
    pub max_level_ts: i64,
    pub name: String,
    pub levels_info: Vec<LevelInfo>,
}

impl Version {
    pub fn new(id: VersionId,
               last_seq: u64,
               name: String,
               levels_info: Vec<LevelInfo>,
               max_level_ts: i64)
               -> Self {
        Self { id, last_seq, name, levels_info, max_level_ts }
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn levels_info(&self) -> &Vec<LevelInfo> {
        &self.levels_info
    }

    // todo:
    pub fn get_ts_overlap(&self, level: u32, ts_min: i64, ts_max: i64) -> Vec<Arc<ColumnFile>> {
        vec![]
    }
}

pub struct SuperVersion {
    pub id: u32,
    pub delta_mut_cache: Arc<RwLock<MemCache>>,
    pub mut_cache: Arc<RwLock<MemCache>>,
    pub immut_cache: Vec<Arc<RwLock<MemCache>>>,
    pub cur_version: Arc<RwLock<Version>>,
    pub opt: Arc<TseriesFamOpt>,
    pub version_id: u64,
}

impl SuperVersion {
    pub fn new(id: u32,
               delta_mut_cache: Arc<RwLock<MemCache>>,
               mut_cache: Arc<RwLock<MemCache>>,
               immut_cache: Vec<Arc<RwLock<MemCache>>>,
               cur_version: Arc<RwLock<Version>>,
               opt: Arc<TseriesFamOpt>,
               version_id: u64)
               -> Self {
        Self { id, delta_mut_cache, mut_cache, immut_cache, cur_version, opt, version_id }
    }
}

pub struct TseriesFamily {
    tf_id: TseriesFamilyId,
    delta_mut_cache: Arc<RwLock<MemCache>>,
    mut_cache: Arc<RwLock<MemCache>>,
    immut_cache: Vec<Arc<RwLock<MemCache>>>,
    // todo: need to del RwLock in memcache
    super_version: Arc<SuperVersion>,
    super_version_id: AtomicU64,
    version: Arc<RwLock<Version>>,
    opts: Arc<TseriesFamOpt>,
    // min seq_no keep in the tsfam memcache
    seq_no: u64,
    immut_ts_min: i64,
    mut_ts_max: i64,
}

// todo: cal ref count
impl TseriesFamily {
    pub async fn new(tf_id: TseriesFamilyId,
                     name: String,
                     cache: MemCache,
                     version: Arc<RwLock<Version>>,
                     opt: TseriesFamOpt)
                     -> Self {
        let mm = Arc::new(RwLock::new(cache));
        let tsf_opt = Arc::new(opt);
        let seq = version.read().await.last_seq;
        let max_level_ts = version.read().await.max_level_ts;
        let delta_mm =
            Arc::new(RwLock::new(MemCache::new(tf_id, GLOBAL_CONFIG.max_memcache_size, seq, true)));
        Self { tf_id,
               seq_no: seq,
               delta_mut_cache: delta_mm.clone(),
               mut_cache: mm.clone(),
               immut_cache: Default::default(),
               super_version: Arc::new(SuperVersion::new(tf_id,
                                                         delta_mm,
                                                         mm,
                                                         Default::default(),
                                                         version.clone(),
                                                         tsf_opt.clone(),
                                                         0)),
               super_version_id: AtomicU64::new(0),
               version,
               opts: tsf_opt,
               immut_ts_min: max_level_ts,
               mut_ts_max: i64::MIN }
    }

    pub async fn switch_memcache(&mut self, cache: Arc<RwLock<MemCache>>) {
        self.super_version.mut_cache.write().await.switch_to_immutable();
        self.immut_cache.push(self.mut_cache.clone());
        self.super_version_id.fetch_add(1, Ordering::SeqCst);
        let vers = SuperVersion::new(self.tf_id,
                                     cache.clone(),
                                     self.delta_mut_cache.clone(),
                                     self.immut_cache.clone(),
                                     self.version.clone(),
                                     self.opts.clone(),
                                     self.super_version_id.load(Ordering::SeqCst));
        self.super_version = Arc::new(vers);
        self.mut_cache = cache;
    }

    pub async fn switch_to_immutable(&mut self) {
        self.super_version.mut_cache.write().await.switch_to_immutable();

        self.immut_cache.push(self.mut_cache.clone());
        self.mut_cache = Arc::from(RwLock::new(MemCache::new(self.tf_id,
                                                             GLOBAL_CONFIG.max_memcache_size,
                                                             self.seq_no,
                                                             false)));
        self.super_version_id.fetch_add(1, Ordering::SeqCst);
        let vers = SuperVersion::new(self.tf_id,
                                     self.delta_mut_cache.clone(),
                                     self.mut_cache.clone(),
                                     self.immut_cache.clone(),
                                     self.version.clone(),
                                     self.opts.clone(),
                                     self.super_version_id.load(Ordering::SeqCst));
        self.super_version = Arc::new(vers);
    }

    async fn wrap_delta_flush_req(&mut self, sender: UnboundedSender<Arc<Mutex<Vec<FlushReq>>>>) {
        let mut req_mem = vec![];
        req_mem.push((self.tf_id, self.delta_mut_cache.clone()));
        self.delta_mut_cache =
            Arc::new(RwLock::new(MemCache::new(self.tf_id,
                                               GLOBAL_CONFIG.max_memcache_size,
                                               self.seq_no,
                                               true)));
        self.super_version_id.fetch_add(1, Ordering::SeqCst);
        let vers = SuperVersion::new(self.tf_id,
                                     self.delta_mut_cache.clone(),
                                     self.mut_cache.clone(),
                                     self.immut_cache.clone(),
                                     self.version.clone(),
                                     self.opts.clone(),
                                     self.super_version_id.load(Ordering::SeqCst));
        self.super_version = Arc::new(vers);
        FLUSH_REQ.lock().push(FlushReq { mems: req_mem, wait_req: 0 });
        info!("delta flush_req send,now req queue len : {}", FLUSH_REQ.lock().len());
        sender.send(FLUSH_REQ.clone()).expect("error send flush req to kvcore");
    }

    fn wrap_flush_req(&mut self, sender: UnboundedSender<Arc<Mutex<Vec<FlushReq>>>>) {
        let mut req_mem = vec![];
        for i in self.immut_cache.iter() {
            req_mem.push((self.tf_id, i.clone()));
        }
        self.immut_cache = vec![];
        self.super_version_id.fetch_add(1, Ordering::SeqCst);
        let vers = SuperVersion::new(self.tf_id,
                                     self.delta_mut_cache.clone(),
                                     self.mut_cache.clone(),
                                     self.immut_cache.clone(),
                                     self.version.clone(),
                                     self.opts.clone(),
                                     self.super_version_id.load(Ordering::SeqCst));
        self.super_version = Arc::new(vers);
        FLUSH_REQ.lock().push(FlushReq { mems: req_mem, wait_req: 0 });
        info!("flush_req send,now req queue len : {}", FLUSH_REQ.lock().len());
        sender.send(FLUSH_REQ.clone()).expect("error send flush req to kvcore");
    }

    // todo(Subsegment) : (&mut self) will case performance regression.we must get writeLock to get
    // version_set when we insert each point
    pub async fn put_mutcache(&mut self,
                              fid: FieldId,
                              val: &[u8],
                              dtype: ValueType,
                              seq: u64,
                              ts: Timestamp,
                              sender: UnboundedSender<Arc<Mutex<Vec<FlushReq>>>>) {
        if self.immut_ts_min == i64::MIN {
            self.immut_ts_min = ts;
        }

        if ts >= self.immut_ts_min {
            if ts > self.mut_ts_max {
                self.mut_ts_max = ts;
            }
            let mut mem = self.super_version.mut_cache.write().await;
            let _ = mem.insert_raw(seq, fid, ts, dtype, val);
        } else {
            let mut delta_mem = self.super_version.delta_mut_cache.write().await;
            let _ = delta_mem.insert_raw(seq, fid, ts, dtype, val);
        }
        if ts >= self.immut_ts_min && !self.delta_mut_cache.read().await.data_cache.is_empty() {
            self.wrap_delta_flush_req(sender.clone()).await
        }

        if self.super_version.mut_cache.read().await.is_full() {
            info!("mut_cache full,switch to immutable");
            self.switch_to_immutable().await;
            if self.immut_cache.len() >= GLOBAL_CONFIG.max_immemcache_num {
                self.immut_ts_min = self.mut_ts_max;
                self.version.write().await.max_level_ts = self.mut_ts_max;
                self.wrap_flush_req(sender.clone());
            }
        }

        if self.super_version.delta_mut_cache.read().await.is_full() {
            self.wrap_delta_flush_req(sender.clone()).await;
        }
    }

    pub async fn delete_cache(&self, time_range: &TimeRange) {
        for i in self.mut_cache.write().await.data_cache.iter_mut() {
            if i.1.overlap(time_range) {
                i.1.delete_data_cell(time_range);
            }
        }
        for i in self.delta_mut_cache.write().await.data_cache.iter_mut() {
            if i.1.overlap(time_range) {
                i.1.delete_data_cell(time_range);
            }
        }
        for memcache in self.immut_cache.iter() {
            for i in memcache.write().await.data_cache.iter_mut() {
                if i.1.overlap(time_range) {
                    i.1.delete_data_cell(time_range);
                }
            }
        }
    }

    pub fn tf_id(&self) -> TseriesFamilyId {
        self.tf_id
    }

    pub fn cache(&self) -> &Arc<RwLock<MemCache>> {
        &self.mut_cache
    }

    pub fn delta_cache(&self) -> &Arc<RwLock<MemCache>> {
        &self.delta_mut_cache
    }

    pub fn im_cache(&self) -> &Vec<Arc<RwLock<MemCache>>> {
        &self.immut_cache
    }

    pub fn version(&self) -> &Arc<RwLock<Version>> {
        &self.version
    }

    pub fn imut_ts_min(&self) -> i64 {
        self.immut_ts_min
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use logger::info;
    use models::ValueType;
    use tokio::sync::{mpsc, RwLock};

    use crate::{
        kv_option::TseriesFamOpt,
        memcache::MemCache,
        tseries_family::{TimeRange, TseriesFamily, Version},
    };

    #[tokio::test]
    pub async fn test_tsf_delete() {
        let tcfg = TseriesFamOpt::default();
        let mut tsf = TseriesFamily::new(0,
                                         "db".to_string(),
                                         MemCache::new(0, 500, 0, false),
                                         Arc::new(RwLock::new(Version::new(0,
                                                                           0,
                                                                           "db".to_string(),
                                                                           vec![],
                                                                           0))),
                                         tcfg).await;
        let (flush_task_sender, flush_task_receiver) = mpsc::unbounded_channel();
        tsf.put_mutcache(0,
                         10_i32.to_be_bytes().as_slice(),
                         ValueType::Integer,
                         0,
                         0,
                         flush_task_sender)
           .await;
        assert_eq!(tsf.mut_cache.read().await.data_cache.get(&0).unwrap().cells.len(), 1);
        tsf.delete_cache(&TimeRange { max_ts: 0, min_ts: 0 }).await;
        assert_eq!(tsf.mut_cache.read().await.data_cache.get(&0).unwrap().cells.len(), 0);
    }
}
