use core::convert::TryFrom;
///https://github.com/Microsoft/uf2/blob/master/hf2.md
use hidapi::HidApi;
use scroll::{ctx, Pread, Pwrite, LE};

#[derive(Debug, Copy, Clone)]
enum CommandId {
    // This command states the current mode of the device:
    BinInfo = 0x0001,
    // Various device information. The result is a character array. See INFO_UF2.TXT in UF2 format for details.
    Info = 0x0002,
    //Reset the device into user-space app. Usually, no response at all will arrive for this command.
    ResetIntoApp = 0x0003,
    //Reset the device into bootloader, usually for flashing. Usually, no response at all will arrive for this command.
    ResetIntoBootloader = 0x0004,
    // When issued in bootloader mode, it has no effect. In user-space mode it causes handover to bootloader. A BININFO command can be issued to verify that.
    StartFlash = 0x0005,
    //Write a single page of flash memory.
    WriteFlashPage = 0x0006,
    //Compute checksum of a number of pages. Maximum value for num_pages is max_message_size / 2 - 2. The checksum algorithm used is CRC-16-CCITT.
    Checksum = 0x0007,
    //Read a number of words from memory. Memory is read word by word (and not byte by byte), and target_addr must be suitably aligned. This is to support reading of special IO regions.
    ReadWords = 0x0008,
    //Dual of READ WORDS, with the same constraints.
    WriteWords = 0x0009,
    //Return internal log buffer if any. The result is a character array.
    Dmesg = 0x0010,
}

#[derive(Debug)]
enum BinInfoMode {
    //bootloader, and thus flashing of user-space programs is allowed
    Bootloader = 0x0001,
    //user-space mode. It also returns the size of flash page size (flashing needs to be done on page-by-page basis), and the maximum size of message. It is always the case that max_message_size >= flash_page_size + 64.
    User = 0x0002,
}

impl TryFrom<u32> for BinInfoMode {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(BinInfoMode::Bootloader),
            2 => Ok(BinInfoMode::User),
            _ => Err(Error::Parse),
        }
    }
}

trait CommanderResult {}

trait Commander<RES: CommanderResult> {
    const ID: CommandId;

    fn transfer(&self, d: &hidapi::HidDevice) -> Result<RES, Error>;
}

struct BinInfo {}

impl Commander<BinInfoResult> for BinInfo {
    const ID: CommandId = CommandId::BinInfo;

    fn transfer(&self, d: &hidapi::HidDevice) -> Result<BinInfoResult, Error> {
        let bitsnbytes = something(Self::ID, d)?;

        let info: BinInfoResult = (bitsnbytes.as_slice()).pread_with::<BinInfoResult>(0, LE)?;

        Ok(info)
    }
}

#[derive(Debug)]
struct BinInfoResult {
    mode: BinInfoMode, //    uint32_t mode;
    flash_page_size: u32,
    flash_num_pages: u32,
    max_message_size: u32,
    family_id: Option<u32>, // optional
}

impl CommanderResult for BinInfoResult {}

impl<'a> ctx::TryFromCtx<'a, scroll::Endian> for BinInfoResult {
    type Error = scroll::Error;
    fn try_from_ctx(this: &'a [u8], le: scroll::Endian) -> Result<(Self, usize), Self::Error> {
        if this.len() < 16 {
            return Err((scroll::Error::Custom("whatever".to_string())).into());
        }

        //does it give me offset somehow??? or just slice appropriately for me?s
        let mut offset = 0;
        let mode: u32 = this.gread_with::<u32>(&mut offset, le)?;
        let mode: Result<BinInfoMode, Error> = BinInfoMode::try_from(mode);
        let mode: BinInfoMode = mode.unwrap();
        let flash_page_size = this.gread_with::<u32>(&mut offset, le)?;
        let flash_num_pages = this.gread_with::<u32>(&mut offset, le)?;
        let max_message_size = this.gread_with::<u32>(&mut offset, le)?;

        //todo, not sure if optional means it would be 0, or would not be included at all
        let family_id = if offset < this.len() {
            Some(this.gread_with::<u32>(&mut offset, le)?)
        } else {
            None
        };

        Ok((
            BinInfoResult {
                mode,
                flash_page_size,
                flash_num_pages,
                max_message_size,
                family_id,
            },
            offset,
        ))
    }
}

struct Info {}
impl Commander<InfoResult> for Info {
    const ID: CommandId = CommandId::Info;

    fn transfer(&self, d: &hidapi::HidDevice) -> Result<InfoResult, Error> {
        let bitsnbytes = something(Self::ID, d)?;

        let info: InfoResult = (bitsnbytes.as_slice()).pread_with::<InfoResult>(0, LE)?;

        Ok(info)
    }
}

#[derive(Debug)]
struct InfoResult {
    info: String,
}
impl CommanderResult for InfoResult {}

impl<'a> ctx::TryFromCtx<'a, scroll::Endian> for InfoResult {
    type Error = scroll::Error;
    fn try_from_ctx(this: &'a [u8], le: scroll::Endian) -> Result<(Self, usize), Self::Error> {
        let mut bytes = vec![0; this.len()];

        let mut offset = 0;
        this.gread_inout_with(&mut offset, &mut bytes, LE)?;

        let info = std::str::from_utf8(&bytes).unwrap();

        Ok((InfoResult { info: info.into() }, offset))
    }
}

struct WriteFlashPageCommand {
    target_addr: u32,
    data: Vec<u8>,
}

struct ChksumPagesCommand {
    target_addr: u32,
    num_pages: u32,
}

//Maximum value for num_pages is max_message_size / 2 - 2. The checksum algorithm used is CRC-16-CCITT.
#[derive(Debug)]
struct ChksumPagesResult {
    chksums: Vec<u16>,
}

struct ReadWordsCommand {
    target_addr: u32,
    num_words: u32,
}

#[derive(Debug)]
struct ReadWordsResult {
    words: Vec<u32>,
}

struct WriteWordsCommand {
    target_addr: u32,
    num_words: u32,
    words: Vec<u32>,
}

// no arguments
#[derive(Debug)]
struct DmesgResult {
    logs: String,
}

struct Command {
    command_id: CommandId, //    uint32_t command_id;
    //arbitrary number set by the host, for example as sequence number. The response should repeat the tag.
    tag: u16,
    //The two reserved bytes in the command should be sent as zero and ignored by the device
    _reserved0: u8,
    _reserved1: u8,
    // data: Vec<u8>,
}

#[derive(Debug)]
struct CommandResponse {
    //arbitrary number set by the host, for example as sequence number. The response should repeat the tag.
    tag: u16,
    status: CommandResponseStatus, //    uint8_t status;

    //additional information In case of non-zero status
    status_info: u8,
    // data: Vec<u8>,
}

impl<'a> ::scroll::ctx::TryIntoCtx<::scroll::Endian> for &'a Command {
    type Error = ::scroll::Error;

    fn try_into_ctx(
        self,
        dst: &mut [u8],
        ctx: ::scroll::Endian,
    ) -> ::scroll::export::result::Result<usize, Self::Error> {
        let mut offset = 0;
        dst.gwrite_with(self.command_id as u32, &mut offset, ctx)?;
        dst.gwrite_with(&self.tag, &mut offset, ctx)?;
        dst.gwrite_with(&self._reserved0, &mut offset, ctx)?;
        dst.gwrite_with(&self._reserved1, &mut offset, ctx)?;

        // for item in &self.data {
        //     dst.gwrite_with(item, &mut offset, ctx)?;
        // }

        Ok(offset)
    }
}

#[derive(Debug, PartialEq)]
enum CommandResponseStatus {
    //command understood and executed correctly
    Success = 0x00,
    //command not understood
    ParseError = 0x01,
    //command execution error
    ExecutionError = 0x02,
}

impl TryFrom<u8> for CommandResponseStatus {
    type Error = Error;

    fn try_from(val: u8) -> Result<Self, Self::Error> {
        match val {
            0 => Ok(CommandResponseStatus::Success),
            1 => Ok(CommandResponseStatus::ParseError),
            2 => Ok(CommandResponseStatus::ExecutionError),
            _ => Err(Error::Parse),
        }
    }
}

#[derive(Debug, PartialEq)]
enum PacketType {
    //Inner packet of a command message
    Inner = 0,
    //Final packet of a command message
    Final = 1,
    //Serial stdout
    StdOut = 2,
    //Serial stderr
    Stderr = 3,
}

impl TryFrom<u8> for PacketType {
    type Error = Error;

    fn try_from(val: u8) -> Result<Self, Self::Error> {
        match val {
            0 => Ok(PacketType::Inner),
            1 => Ok(PacketType::Final),
            2 => Ok(PacketType::StdOut),
            3 => Ok(PacketType::Stderr),
            _ => Err(Error::Parse),
        }
    }
}

// doesnt know what the dta is supposed to be decoded as
// thats linked via the seq number outside,  so we cant decode here
impl<'a> ctx::TryFromCtx<'a, scroll::Endian> for CommandResponse {
    type Error = scroll::Error;
    fn try_from_ctx(this: &'a [u8], le: scroll::Endian) -> Result<(Self, usize), Self::Error> {
        println!("len{:?}", this.len());

        if this.len() < 8 {
            return Err((scroll::Error::Custom("whatever".to_string())).into());
        }

        let mut offset = 0;
        let tag = this.gread_with::<u16>(&mut offset, le)?;
        let status: u8 = this.gread_with::<u8>(&mut offset, le)?;
        let status = CommandResponseStatus::try_from(status).unwrap();
        let status_info = this.gread_with::<u8>(&mut offset, le)?;
        // let mut data: Vec<u8> = vec![];
        // this.gread_inout_with(&mut offset, &mut data, le)?;

        // for item in &self.data {
        //     dst.gwrite_with(item, &mut offset, ctx)?;
        // }

        Ok((
            CommandResponse {
                tag,
                status,
                status_info,
                // data,
            },
            offset,
        ))
    }
}

fn something(id: CommandId, d: &hidapi::HidDevice) -> Result<Vec<u8>, Error> {
    let mut seq: u16 = 1;

    let command = Command {
        command_id: id,
        //arbitrary number set by the host, for example as sequence number. The response should repeat the tag.
        tag: seq,
        //The two reserved bytes in the command should be sent as zero and ignored by the device
        _reserved0: 0,
        _reserved1: 0,
        // data: vec![],
    };
    let buffer = &mut [0; 64];

    let bytes = buffer.pwrite_with(&command, 1, LE)?;
    buffer[0] = (PacketType::Final as u8) << 6 | bytes as u8;

    d.write(buffer)?;

    let mut bitsnbytes: Vec<u8> = vec![];

    //if inner, need to buffer more packets
    let mut ptype = PacketType::Inner;
    while ptype == PacketType::Inner {
        d.read(buffer)?;
        println!("Receive response: {:02X?}", &buffer[..]);

        ptype = PacketType::try_from(buffer[0] >> 6).unwrap();
        let len: usize = (buffer[0] & 0x3F) as usize;

        //skip the header byte
        bitsnbytes.extend_from_slice(&buffer[1..=len]);
    }

    let mut offset = 0;

    let resp = bitsnbytes
        .as_slice()
        .gread_with::<CommandResponse>(&mut offset, LE)?;

    if resp.status != CommandResponseStatus::Success {
        return Err(Error::MalformedRequest);
    }

    if resp.tag != seq {
        return Err(Error::Sequence);
    }

    Ok(bitsnbytes[offset..].to_vec())
}

#[derive(Clone, Debug)]
pub(crate) enum Error {
    Parse,
    MalformedRequest,
    Execution,
    Sequence,
    Transmission,
}

impl From<hidapi::HidError> for Error {
    fn from(_err: hidapi::HidError) -> Self {
        Error::Transmission
    }
}

impl From<scroll::Error> for Error {
    fn from(_err: scroll::Error) -> Self {
        Error::Parse
    }
}

// enum UF2Result {
//     BinInfo(BinInfoResult),
//     Info(InfoResult),
//     ChksumPages(ChksumPagesResult),
//     ReadWords(ReadWordsResult),
//     Dmesg(DmesgResult),
// }

fn main() -> Result<(), Error> {
    let api = HidApi::new().expect("Couldn't find system usb");

    let d = api.open(0x239A, 0x003D).expect("Couldn't find usb device");

    let bininfo: BinInfoResult = BinInfo {}.transfer(&d)?;
    println!("{:?}", bininfo);

    let info: InfoResult = Info {}.transfer(&d)?;
    println!("{:?}", info);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packetize() {
        let message = vec![
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0xD0, 0x09, 0x0A, 0x0B, 0x0C, 0x0D,
            0x0E, 0x0F, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
        ];
        // Packet 0: 83 01 02 03 AB FF FF FF
        // Packet 1: 85 04 05 06 07 08
        // Packet 2: 80 DE 42 42 42 42 FF FF
        // Packet 3: D0 09 0A 0B 0C 0D 0E 0F 10 11 12 13 14 15 16 17 FF FF FF

        unimplemented!();
    }
}
