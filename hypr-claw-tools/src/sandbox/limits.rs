use rlimit::{setrlimit, Resource};

const MEMORY_LIMIT: u64 = 512 * 1024 * 1024; // 512MB
const CPU_LIMIT: u64 = 60; // 60 seconds
const FILE_SIZE_LIMIT: u64 = 100 * 1024 * 1024; // 100MB
const NPROC_LIMIT: u64 = 10; // Max 10 processes
const NOFILE_LIMIT: u64 = 100; // Max 100 file descriptors

pub struct ResourceLimits;

impl ResourceLimits {
    pub fn apply() -> Result<(), Box<dyn std::error::Error>> {
        // Memory limit
        setrlimit(Resource::AS, MEMORY_LIMIT, MEMORY_LIMIT)?;

        // CPU time limit
        setrlimit(Resource::CPU, CPU_LIMIT, CPU_LIMIT)?;

        // File size limit
        setrlimit(Resource::FSIZE, FILE_SIZE_LIMIT, FILE_SIZE_LIMIT)?;

        // Process limit
        setrlimit(Resource::NPROC, NPROC_LIMIT, NPROC_LIMIT)?;

        // File descriptor limit
        setrlimit(Resource::NOFILE, NOFILE_LIMIT, NOFILE_LIMIT)?;

        Ok(())
    }
}
