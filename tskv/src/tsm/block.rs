use std::cell::Cell;

use models::ValueType;
use protos::models::FieldType;

use super::coders;
use crate::{
    error::{Error, Result},
    memcache::{BoolCell, Byte, DataType, F64Cell, I64Cell, StrCell, U64Cell},
};

#[derive(Debug, Clone, PartialEq)]
pub enum DataBlock {
    U64 { ts: Vec<i64>, val: Vec<u64> },
    I64 { ts: Vec<i64>, val: Vec<i64> },
    Str { ts: Vec<i64>, val: Vec<Byte> },
    F64 { ts: Vec<i64>, val: Vec<f64> },
    Bool { ts: Vec<i64>, val: Vec<bool> },
}

impl DataBlock {
    pub fn new(size: usize, field_type: ValueType) -> Self {
        match field_type {
            ValueType::Unsigned => {
                Self::U64 { ts: Vec::with_capacity(size), val: Vec::with_capacity(size) }
            },
            ValueType::Integer => {
                Self::I64 { ts: Vec::with_capacity(size), val: Vec::with_capacity(size) }
            },
            ValueType::Float => {
                Self::F64 { ts: Vec::with_capacity(size), val: Vec::with_capacity(size) }
            },
            ValueType::String => {
                Self::Str { ts: Vec::with_capacity(size), val: Vec::with_capacity(size) }
            },
            ValueType::Boolean => {
                Self::Bool { ts: Vec::with_capacity(size), val: Vec::with_capacity(size) }
            },
            ValueType::Unknown => {
                todo!()
            },
        }
    }

    /// Inserts new timestamp and value wrapped by `DataType` to this `DataBlock`.
    pub fn insert(&mut self, data: &DataType) {
        match data {
            DataType::Bool(item) => {
                if let Self::Bool { ts, val, .. } = self {
                    ts.push(item.ts);
                    val.push(item.val);
                }
            },
            DataType::U64(item) => {
                if let Self::U64 { ts, val, .. } = self {
                    ts.push(item.ts);
                    val.push(item.val);
                }
            },
            DataType::I64(item) => {
                if let Self::I64 { ts, val, .. } = self {
                    ts.push(item.ts);
                    val.push(item.val);
                }
            },
            DataType::Str(item) => {
                if let Self::Str { ts, val, .. } = self {
                    ts.push(item.ts);
                    val.push(item.val.clone());
                }
            },
            DataType::F64(item) => {
                if let Self::F64 { ts, val, .. } = self {
                    ts.push(item.ts);
                    val.push(item.val);
                }
            },
        }
    }

    /// Returns (`timestamp[start]`, `timestamp[end]`) from this `DataBlock` at the specified
    /// indexes.
    pub fn time_range(&self, start: usize, end: usize) -> (i64, i64) {
        match self {
            DataBlock::U64 { ts, .. } => (ts[start].to_owned(), ts[end - 1].to_owned()),
            DataBlock::I64 { ts, .. } => (ts[start].to_owned(), ts[end - 1].to_owned()),
            DataBlock::Str { ts, .. } => (ts[start].to_owned(), ts[end - 1].to_owned()),
            DataBlock::F64 { ts, .. } => (ts[start].to_owned(), ts[end - 1].to_owned()),
            DataBlock::Bool { ts, .. } => (ts[start].to_owned(), ts[end - 1].to_owned()),
        }
    }

    /// Inserts new timestamps and values wrapped by `&[DataType]` to this `DataBlock`.
    pub fn batch_insert(&mut self, cells: &[DataType]) {
        for iter in cells.iter() {
            self.insert(iter);
        }
    }

    /// Returns the length of the timestamps array of this `DataBlock`.
    pub fn len(&self) -> usize {
        match &self {
            Self::U64 { ts, .. } => ts.len(),
            Self::I64 { ts, .. } => ts.len(),
            Self::F64 { ts, .. } => ts.len(),
            Self::Str { ts, .. } => ts.len(),
            Self::Bool { ts, .. } => ts.len(),
        }
    }

    /// Returns the `ValueType` by this `DataBlock` variant.
    pub fn field_type(&self) -> ValueType {
        match &self {
            DataBlock::U64 { .. } => ValueType::Unsigned,
            DataBlock::I64 { .. } => ValueType::Integer,
            DataBlock::Str { .. } => ValueType::String,
            DataBlock::F64 { .. } => ValueType::Float,
            DataBlock::Bool { .. } => ValueType::Boolean,
        }
    }

    /// Returns a new `DataType` by this `DataBlock` variant.
    pub fn get_type(&self) -> DataType {
        match &self {
            DataBlock::U64 { .. } => DataType::U64(U64Cell::default()),
            DataBlock::I64 { .. } => DataType::I64(I64Cell::default()),
            DataBlock::Str { .. } => DataType::Str(StrCell::default()),
            DataBlock::F64 { .. } => DataType::F64(F64Cell::default()),
            DataBlock::Bool { .. } => DataType::Bool(BoolCell::default()),
        }
    }

    /// Returns a slice containing the entire timestamps of this `DataBlock`.
    pub fn ts(&self) -> &[i64] {
        match self {
            DataBlock::U64 { ts, .. } => ts.as_slice(),
            DataBlock::I64 { ts, .. } => ts.as_slice(),
            DataBlock::Str { ts, .. } => ts.as_slice(),
            DataBlock::F64 { ts, .. } => ts.as_slice(),
            DataBlock::Bool { ts, .. } => ts.as_slice(),
        }
    }

    /// Returns whether all elements of this `DataBlock` has been iterated
    /// ( `DataBlock::*index == DataBlock::ts.len()` )
    pub fn is_empty(&self) -> bool {
        match &self {
            DataBlock::U64 { ts, .. } => ts.len() > 0,
            DataBlock::I64 { ts, .. } => ts.len() > 0,
            DataBlock::Str { ts, .. } => ts.len() > 0,
            DataBlock::F64 { ts, .. } => ts.len() > 0,
            DataBlock::Bool { ts, .. } => ts.len() > 0,
        }
    }

    /// Returns the next `DataType` at the inner index.
    // pub fn next(&mut self) -> Option<DataType> {
    //     if self.is_empty() {
    //         return None;
    //     }
    //     match self {
    //         DataBlock::U64 { index, ts, val } => {
    //             let i = *index as usize;
    //             *index += 1;
    //             Some(DataType::U64(U64Cell { ts: ts[i], val: val[i] }))
    //         },
    //         DataBlock::I64 { index, ts, val } => {
    //             let i = *index as usize;
    //             *index += 1;
    //             Some(DataType::I64(I64Cell { ts: ts[i], val: val[i] }))
    //         },
    //         DataBlock::Str { index, ts, val } => {
    //             let i = *index as usize;
    //             *index += 1;
    //             Some(DataType::Str(StrCell { ts: ts[i], val: val[i].clone() }))
    //         },
    //         DataBlock::F64 { index, ts, val } => {
    //             let i = *index as usize;
    //             *index += 1;
    //             Some(DataType::F64(F64Cell { ts: ts[i], val: val[i] }))
    //         },
    //         DataBlock::Bool { index, ts, val } => {
    //             let i = *index as usize;
    //             *index += 1;
    //             Some(DataType::Bool(BoolCell { ts: ts[i], val: val[i] }))
    //         },
    //     }
    // }

    /// Returns the (ts, val) wrapped by `DataType` at the index 'i'
    pub fn get(&self, i: usize) -> Option<DataType> {
        match self {
            DataBlock::U64 { ts, val, .. } => {
                if ts.len() <= i {
                    None
                } else {
                    dbg!(ts.len());
                    Some(DataType::U64(U64Cell { ts: ts[i], val: val[i] }))
                }
            },
            DataBlock::I64 { ts, val, .. } => {
                if ts.len() <= i {
                    None
                } else {
                    Some(DataType::I64(I64Cell { ts: ts[i], val: val[i] }))
                }
            },
            DataBlock::Str { ts, val, .. } => {
                if ts.len() <= i {
                    None
                } else {
                    Some(DataType::Str(StrCell { ts: ts[i], val: val[i].clone() }))
                }
            },
            DataBlock::F64 { ts, val, .. } => {
                if ts.len() <= i {
                    None
                } else {
                    Some(DataType::F64(F64Cell { ts: ts[i], val: val[i] }))
                }
            },
            DataBlock::Bool { ts, val, .. } => {
                if ts.len() <= i {
                    None
                } else {
                    Some(DataType::Bool(BoolCell { ts: ts[i], val: val[i] }))
                }
            },
        }
    }

    /// Set the (ts, val) wrapped by `DataType` at the index 'i'
    pub fn set(&mut self, i: usize, data_type: DataType) {
        match (self, data_type) {
            (DataBlock::U64 { ts, val, .. }, DataType::U64(c)) => {
                ts[i] = c.ts;
                val[i] = c.val;
            },
            (DataBlock::I64 { ts, val, .. }, DataType::I64(c)) => {
                ts[i] = c.ts;
                val[i] = c.val;
            },
            (DataBlock::Str { ts, val, .. }, DataType::Str(c)) => {
                ts[i] = c.ts;
                val[i] = c.val;
            },
            (DataBlock::F64 { ts, val, .. }, DataType::F64(c)) => {
                ts[i] = c.ts;
                val[i] = c.val;
            },
            (DataBlock::Bool { ts, val, .. }, DataType::Bool(c)) => {
                ts[i] = c.ts;
                val[i] = c.val;
            },
            _ => {},
        }
    }

    /// Append a `DataBlock` into this `DataBlock`, sorted by timestamp,
    /// if two (timestamp, value) conflict with the same timestamp, use the last value.
    pub fn append(&mut self, other: &Self) {
        if other.is_empty() {
            return;
        }
        if self.field_type() != other.field_type() {
            return;
        }
        let (smin_ts, smax_ts) = self.time_range(0, self.len());
        let (min_ts, max_ts) = other.time_range(0, other.len());

        let i_ts_sli = self.ts();
        let ts_sli = other.ts();
        let mut new_blk = Self::new(self.len() + other.len(), self.field_type());
        let (mut i, mut j, mut k) = (0_usize, 0_usize, 0_usize);
        while i < i_ts_sli.len() && j < ts_sli.len() {
            match i_ts_sli[i].cmp(&ts_sli[j]) {
                std::cmp::Ordering::Less => {
                    new_blk.set(k, self.get(i).expect("checked index i"));
                    i += 1;
                },
                std::cmp::Ordering::Equal => {
                    new_blk.set(k, other.get(j).expect("checked index j"));
                    i += 1;
                    j += 1;
                },
                std::cmp::Ordering::Greater => {
                    new_blk.set(k, other.get(j).expect("checked index j"));
                    j += 1;
                },
            }
            k += 1;
        }
    }

    /// Merges many `DataBlock`s into one `DataBlock`, sorted by timestamp,
    /// if many (timestamp, value) conflict with the same timestamp, use the last value.
    pub fn merge_blocks(mut blocks: Vec<Self>) -> Self {
        if blocks.len() == 1 {
            return blocks.remove(0);
        }

        let mut res =
            Self::new(blocks.first().unwrap().len(), blocks.first().unwrap().field_type());
        // [(DataBlock)]
        let mut buf = vec![None; blocks.len()];
        let mut offsets = vec![0_usize; blocks.len()];
        loop {
            match Self::rebuild_vec(&mut blocks, &mut buf, &mut offsets) {
                Some(min) => {
                    let mut data = None;
                    for item in &mut buf {
                        if let Some(it) = item {
                            if it.timestamp() == min {
                                data = item.take();
                            }
                        }
                    }
                    if let Some(it) = data {
                        res.insert(&it);
                    }
                },
                None => return res,
            }
        }
    }

    /// Extract `DataBlock`s to `DataType`s,
    /// returns the minimum timestamp in a series of `DataBlock`s
    fn rebuild_vec(blocks: &mut [Self],
                   dst: &mut Vec<Option<DataType>>,
                   offsets: &mut [usize])
                   -> Option<i64> {
        let mut min_ts = None;
        for (i, (block, dst)) in blocks.iter_mut().zip(dst).enumerate() {
            if dst.is_none() {
                *dst = block.get(offsets[i]);
                offsets[i] += 1;
            }
            dbg!(&dst);

            if let Some(pair) = dst {
                match min_ts {
                    Some(min) => {
                        if pair.timestamp() < min {
                            min_ts = Some(pair.timestamp());
                        }
                    },
                    None => min_ts = Some(pair.timestamp()),
                }
            };
        }
        min_ts
    }

    // todo:
    /// Encodes timestamps and values of this `DataBlock` to bytes.
    pub fn encode(&self, start: usize, end: usize) -> Result<(Vec<u8>, Vec<u8>)> {
        let mut ts_buf = vec![];
        let mut data_buf = vec![];
        match self {
            DataBlock::Bool { ts, val, .. } => {
                coders::timestamp::encode(&ts[start..end], &mut ts_buf)
                    .map_err(|e| Error::WriteTsmErr { reason: e.to_string() })?;
                coders::boolean::encode(&val[start..end], &mut data_buf)
                    .map_err(|e| Error::WriteTsmErr { reason: e.to_string() })?;
            },
            DataBlock::U64 { ts, val, .. } => {
                coders::timestamp::encode(&ts[start..end], &mut ts_buf)
                    .map_err(|e| Error::WriteTsmErr { reason: e.to_string() })?;
                coders::unsigned::encode(&val[start..end], &mut data_buf)
                    .map_err(|e| Error::WriteTsmErr { reason: e.to_string() })?;
            },
            DataBlock::I64 { ts, val, .. } => {
                coders::timestamp::encode(&ts[start..end], &mut ts_buf)
                    .map_err(|e| Error::WriteTsmErr { reason: e.to_string() })?;
                coders::integer::encode(&val[start..end], &mut data_buf)
                    .map_err(|e| Error::WriteTsmErr { reason: e.to_string() })?;
            },
            DataBlock::Str { ts, val, .. } => {
                coders::timestamp::encode(&ts[start..end], &mut ts_buf)
                    .map_err(|e| Error::WriteTsmErr { reason: e.to_string() })?;
                let strs: Vec<&[u8]> = val.iter().map(|str| &str[..]).collect();
                coders::string::encode(&strs[start..end], &mut data_buf)
                    .map_err(|e| Error::WriteTsmErr { reason: e.to_string() })?;
            },
            DataBlock::F64 { ts, val, .. } => {
                coders::timestamp::encode(&ts[start..end], &mut ts_buf)
                    .map_err(|e| Error::WriteTsmErr { reason: e.to_string() })?;
                coders::float::encode(&val[start..end], &mut data_buf)
                    .map_err(|e| Error::WriteTsmErr { reason: e.to_string() })?;
            },
        }
        Ok((ts_buf, data_buf))
    }

    pub fn decode() {}
}

#[cfg(test)]
mod test {
    use crate::tsm::DataBlock;

    #[test]
    fn test_merge_blocks() {
        let res = DataBlock::merge_blocks(vec![DataBlock::U64 { ts: vec![1, 2, 3, 4, 5],
                                                                val: vec![10, 20, 30, 40, 50] },
                                               DataBlock::U64 { ts: vec![2, 3, 4],
                                                                val: vec![12, 13, 15] },]);

        assert_eq!(res, DataBlock::U64 { ts: vec![1, 2, 3, 4, 5], val: vec![10, 12, 13, 15, 50] },);
    }

    #[test]
    fn test_append_block() {
        // let b1 = DataBlock
    }
}
