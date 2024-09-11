#[derive(Debug)]
pub enum ChannelDataType {
    RemoveClient(u64),
    Other,
}
