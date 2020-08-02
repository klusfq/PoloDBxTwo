use std::fmt;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write, Read};
use std::sync::Weak;
use std::sync::Arc;

use super::db::DbContext;

enum PageType {
    Undefined = 0,

    FileHeader,

    Collection,

    BTreeNode,

}

#[derive(Debug)]
pub struct RawPage {
    page_id:       u32,
    data:          Vec<u8>,
    pos:           u32,
}

#[derive(Debug, Clone)]
pub struct SpaceNotEnough;

impl fmt::Display for SpaceNotEnough {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "page space is not enough")
    }
}

impl RawPage {

    pub fn new(page_id: u32, size: u32) -> RawPage {
        let mut v: Vec<u8> = Vec::new();
        v.resize(size as usize, 0);
        RawPage {
            page_id,
            data: v,
            pos: 0,
        }
    }

    pub fn put(&mut self, data: &[u8]) -> Result<(), SpaceNotEnough> {
        if data.len() + self.pos as usize > self.data.len() {
            return Err(SpaceNotEnough);
        }

        unsafe {
            self.data.as_mut_ptr().offset(self.pos as isize).copy_from(data.as_ptr(), data.len());
        }

        self.pos += data.len() as u32;

        Ok(())
    }

    pub fn put_str(&mut self, str: &str) -> Result<(), SpaceNotEnough> {
        if str.len() + self.pos as usize > self.data.len() {
            return Err(SpaceNotEnough);
        }

        unsafe {
            self.data.as_mut_ptr().offset(self.pos as isize).copy_from(str.as_ptr(), str.len());
        }
        self.pos += str.len() as u32;

        Ok(())
    }

    pub fn get_u8(&self, pos: usize) -> u8 {
        self.data[pos]
    }

    #[inline]
    pub fn put_u8(&mut self, data: u8) {
        self.data[self.pos as usize] = data
    }

    #[inline]
    pub fn put_u16(&mut self, data: u16) -> Result<(), SpaceNotEnough> {
        let data_be = data.to_le_bytes();
        self.put(&data_be)
    }

    #[inline]
    pub fn get_u16(&self, pos: usize) -> u16 {
        let mut buffer: [u8; 2] = [0; 2];
        buffer.copy_from_slice(&self.data[pos..(pos+2)]);
        u16::from_be_bytes(buffer)
    }

    #[inline]
    pub fn put_u32(&mut self, data: u32) -> Result<(), SpaceNotEnough> {
        let data_be = data.to_be_bytes();
        self.put(&data_be)
    }

    #[inline]
    pub fn get_u32(&self, pos: usize) -> u32 {
        let mut buffer: [u8; 4] = [0; 4];
        buffer.copy_from_slice(&self.data[pos..(pos + 4)]);
        u32::from_be_bytes(buffer)
    }

    #[inline]
    pub fn put_u64(&mut self, data: u64) -> Result<(), SpaceNotEnough> {
        let data_be = data.to_be_bytes();
        self.put(&data_be)
    }

    #[inline]
    pub fn get_u64(&self, pos: usize) -> u64 {
        let mut buffer: [u8; 8] = [0; 8];
        buffer.copy_from_slice(&self.data[pos..(pos + 8)]);
        u64::from_be_bytes(buffer)
    }

    pub fn sync_to_file(&self, file: &mut File, offset: u64) -> std::io::Result<()> {
        file.seek(SeekFrom::Start(offset))?;
        file.write(self.data.as_slice())?;
        Ok(())
    }

    pub fn read_from_file(&mut self, file: &mut File, offset: u64) -> std::io::Result<()> {
        file.seek(SeekFrom::Start(offset))?;
        file.read(self.data.as_mut_slice())?;
        Ok(())
    }

    #[inline]
    pub fn seek(&mut self, pos: u32) {
        self.pos = pos;
    }

    #[inline]
    pub fn len(&self) {
        self.data.len();
    }

}

struct FreeList {
    free_list_page_id:   u32,
    data:                Vec<u32>,
}

static FREE_LIST_OFFSET: usize = 2048;

impl FreeList {

    fn new() -> FreeList {
        FreeList {
            free_list_page_id: 0,
            data: Vec::new(),
        }
    }

    fn from_raw(raw_page: &RawPage) -> FreeList {
        let size = raw_page.get_u32(FREE_LIST_OFFSET);
        let free_list_page_id = raw_page.get_u32(FREE_LIST_OFFSET + 4);

        let mut data: Vec<u32> = Vec::new();
        data.resize(size as usize, 0);

        for i in 0..size {
            let offset = FREE_LIST_OFFSET + 8 + (i * 4) as usize;
            data.insert(i as usize, raw_page.get_u32(offset));
        }

        FreeList {
            free_list_page_id,
            data,
        }
    }
    
}

/**
 * Offset 0 (32 bytes) : "PipeappleDB Format v0.1";
 * Offset 32 (8 bytes) : Version 0.0.0.0;
 * Offset 40 (4 bytes) : SectorSize;
 * Offset 44 (4 bytes) : PageSize;
 *
 * Free list offset: 2048;
 * | 4b   | 4b                  | 4b     | 4b    | ... |
 * | size | free list page link | free 1 | free2 | ... |
 */
pub mod header_page_utils {
    use crate::page::RawPage;

    static HEADER_DESP: &str       = "PipeappleDB Format v0.1";
    static SECTOR_SIZE_OFFSET: u32 = 40;
    static PAGE_SIZE_OFFSET: u32   = 44;
    static FREE_LIST_OFFSET: u32   = 2048;

    pub fn init(page: &mut RawPage) {
        set_title(page, HEADER_DESP);
        set_version(page, &[0, 0, 0, 0]);
        set_sector_size(page, 4096);
        set_page_size(page, 4096);
    }

    pub fn set_title(page: &mut RawPage, title: &str) {
        page.seek(0);
        let _ = page.put_str(title);
    }

    pub fn get_title(page: &RawPage) -> String {
        let mut zero_pos: i32 = -1;
        for i in 0..32 {
            if page.data[i] == 0 {
                zero_pos = i as i32;
                break;
            }
        }

        if zero_pos < 0 {
            panic!("can not find a zero")
        }

        let title = String::from_utf8_lossy(&page.data[0..(zero_pos as usize)]);
        title.to_string()
    }

    pub fn set_version(page: &mut RawPage, version: &[u8]) {
        page.seek(32);
        let _ = page.put(version);
    }

    pub fn get_version(page: &RawPage) -> [u8; 4] {
        let mut version: [u8; 4] = [0; 4];
        for i in 0..4 {
            version[i] = page.data[32 + i];
        }
        version
    }

    pub fn set_sector_size(page: &mut RawPage, sector_size: u32) {
        page.seek(SECTOR_SIZE_OFFSET);
        let _ = page.put_u32(sector_size);
    }

    pub fn get_sector_size(page: &RawPage) -> u32 {
        page.get_u32(SECTOR_SIZE_OFFSET as usize)
    }

    pub fn set_page_size(page: &mut RawPage, page_size: u32) {
        page.seek(PAGE_SIZE_OFFSET);
        let _ = page.put_u32(page_size);
    }

    pub fn get_page_size(page: &RawPage) -> u32 {
        page.get_u32(PAGE_SIZE_OFFSET as usize)
    }

    pub fn get_free_list_size(page: &RawPage) -> u32 {
        page.get_u32(FREE_LIST_OFFSET as usize)
    }

    pub fn set_free_list_size(page: &mut RawPage, size: u32) {
        page.seek(FREE_LIST_OFFSET);
        let _ = page.put_u32(size);
    }

    pub fn get_free_list_content(page: &RawPage, index: u32) -> u32 {
        let offset = index * 4 + FREE_LIST_OFFSET + 8;
        page.get_u32(offset as usize)
    }

    #[cfg(test)]
    mod tests {
        // use crate::page::HeaderPage;

        use crate::page::RawPage;
        use crate::page::header_page_utils::*;

        #[test]
        fn parse_and_gen() {
            let mut raw_page = RawPage::new(0, 4096);

            let title = "test title";
            set_title(&mut raw_page, title);
            assert_eq!(get_title(&raw_page), title);

            let test_sector_size = 111;
            set_sector_size(&mut raw_page, test_sector_size);
            assert_eq!(get_sector_size(&raw_page), test_sector_size);

            let test_page_size = 222;
            set_page_size(&mut raw_page, test_page_size);
            assert_eq!(get_page_size(&raw_page), test_page_size);
        }

    }

}

#[repr(u8)]
pub enum ContentPageType {
    Undefined = 0,
    FileHeader,
    Collection,
    BTreeNode,
}

impl ContentPageType {

    pub fn from_u8(data: u8) -> ContentPageType {
        match data {
            0 => ContentPageType::Undefined,
            1 => ContentPageType::FileHeader,
            2 => ContentPageType::Collection,
            3 => ContentPageType::BTreeNode,
            _ => panic!("unknown content type")
        }
    }

}

pub struct CollectionPage {
    ty: ContentPageType,  // u16
}

pub struct ContentPage {
    ty: ContentPageType,  // u16
    right_pid: u32,       // u32
    __reserved: u16,      // u16
}

static CONTENT_TY_OFFSET: u32   = 16;

// magic key:  u16
// ty:         u8
// reserve:    u8
// next_pid:   u32
// data:       offset 64
pub struct ContentPageWrapper {
    ctx:        Weak<DbContext>,
    raw:        RawPage,
    start_page_id:  u32,
}

impl ContentPageWrapper {

    pub fn new(weak_ctx: Weak<DbContext>, page: RawPage) -> ContentPageWrapper {
        // let ctx = weak_ctx.upgrade().expect("get ctx failed");
        let start_page_id = page.page_id;
        ContentPageWrapper {
            ctx: weak_ctx,
            raw: page,
            start_page_id,
        }
    }

    pub fn magic_key(&self) {
        self.raw.get_u16(0);
    }

    pub fn set_magic_key(&mut self, key: u16) {
        self.raw.seek(0);
        self.raw.put_u16(key).expect("set magic key failed");
    }

    pub fn set_next_page_id(&mut self, next_pid: u32) {
        self.raw.seek(32);
        self.raw.put_u32(next_pid).expect("set page id failed")
    }

    pub fn get_next_page_id(&self) -> u32 {
        self.raw.get_u32(32)
    }

    pub fn ty(&self) -> ContentPageType {
        let ty8 = self.raw.get_u8(CONTENT_TY_OFFSET as usize);
        ContentPageType::from_u8(ty8)
    }

    pub fn set_ty(&mut self, ty: ContentPageType) {
        let ty8 = ty as u8;
        self.raw.seek(CONTENT_TY_OFFSET);
        self.raw.put_u8(ty8);
    }

}