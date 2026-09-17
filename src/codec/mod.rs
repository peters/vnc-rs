mod raw;
mod zlib;
mod zrle;
pub(crate) use raw::Decoder as RawDecoder;
pub(crate) use zrle::Decoder as ZrleDecoder;

fn initialized_vec(len: usize) -> Vec<u8> {
    vec![0; len]
}
