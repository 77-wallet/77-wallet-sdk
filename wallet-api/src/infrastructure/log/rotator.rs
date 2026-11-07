use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::PathBuf,
};

struct InnerWriter {
    base_path: PathBuf,
    current_file: File,
}

pub struct SizeRotatingWriter {
    inner: InnerWriter,
}

impl SizeRotatingWriter {
    pub const MAX_FILES: usize = 10;
    pub const MAX_SIZE: u64 = 1024 * 1024 * 32;

    pub fn new(base_path: PathBuf) -> Result<Self, crate::error::system::SystemError> {
        let file = Self::create_file(base_path.clone())?;
        Ok(Self { inner: InnerWriter { base_path, current_file: file } })
    }

    fn create_file(base_path: PathBuf) -> io::Result<File> {
        let file_existed = base_path.exists();
        let mut file = OpenOptions::new().create(true).append(true).open(&base_path)?;

        // 如果是新文件或者空文件，写入时间戳
        if !file_existed {
            let timestamp = chrono::Local::now().timestamp();
            writeln!(file, "{}", timestamp)?;
        }
        Ok(file)
    }

    fn rotate_files(&self, max_files: usize) -> io::Result<()> {
        let oldest = self.inner.base_path.with_extension(format!("{}.txt", max_files));

        if oldest.exists() {
            fs::remove_file(&oldest)?;
        }

        for i in (1..=max_files - 1).rev() {
            let from = self.inner.base_path.with_extension(format!("{}.txt", i));
            let to = self.inner.base_path.with_extension(format!("{}.txt", i + 1));
            if from.exists() {
                fs::rename(from, to)?;
            }
        }

        let rotated = self.inner.base_path.with_extension("1.txt");
        if self.inner.base_path.exists() {
            fs::rename(self.inner.base_path.clone(), rotated)?;
        }

        Ok(())
    }
}

impl Write for SizeRotatingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let metadata = self.inner.current_file.metadata()?;
        if metadata.len() >= Self::MAX_SIZE {
            self.rotate_files(Self::MAX_FILES)?;
            self.inner.current_file = Self::create_file(self.inner.base_path.clone())?;
        }

        self.inner.current_file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.current_file.flush()
    }
}
