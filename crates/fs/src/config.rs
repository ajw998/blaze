/// Batch size for sending records through the channel.
/// Larger batches reduce channel overhead but increase latency.
pub const BATCH_SIZE: usize = 64;
