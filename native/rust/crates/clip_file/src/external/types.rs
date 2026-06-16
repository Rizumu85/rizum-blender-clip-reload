#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalTileBlob {
    pub external_id: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalTileBlock {
    pub tile_index: usize,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalTileBlocks {
    pub external_id: String,
    pub blocks: Vec<ExternalTileBlock>,
}
