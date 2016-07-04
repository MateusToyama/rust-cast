#![feature(custom_derive, plugin)]
#![plugin(serde_macros)]

extern crate byteorder;
#[macro_use]
extern crate log;
extern crate openssl;
extern crate protobuf;
extern crate serde;
extern crate serde_json;

pub mod cast;
pub mod errors;
mod utils;
mod message_manager;
pub mod channels;

use std::cell::RefCell;
use std::net::TcpStream;
use std::rc::Rc;

use openssl::ssl::{SslContext, SslStream, SslMethod};

use channels::heartbeat::{HeartbeatChannel, HeartbeatResponse};
use channels::connection::{ConnectionChannel, ConnectionResponse};
use channels::receiver::{ReceiverChannel, ReceiverResponse};
use channels::media::{MediaChannel, MediaResponse};

use errors::Error;

use message_manager::MessageManager;

const DEFAULT_SENDER_ID: &'static str = "sender-0";
const DEFAULT_RECEIVER_ID: &'static str = "receiver-0";

pub enum ChannelMessage<'a> {
    Connection(ConnectionResponse),
    Hearbeat(HeartbeatResponse),
    Media(MediaResponse<'a>),
    Receiver(ReceiverResponse),
}

pub struct Chromecast {
    stream: Rc<RefCell<SslStream<TcpStream>>>,

    pub heartbeat: HeartbeatChannel<SslStream<TcpStream>>,
    pub connection: ConnectionChannel<SslStream<TcpStream>>,
    pub receiver: ReceiverChannel<SslStream<TcpStream>>,
    pub media: MediaChannel<SslStream<TcpStream>>,
}

impl Chromecast {
    pub fn connect(host: String, port: u16) -> Result<Chromecast, Error> {
        debug!("Establishing connection with Chromecast at {}:{}...", host, port);

        let ssl_context = try!(SslContext::new(SslMethod::Sslv23));
        let tcp_stream = try!(TcpStream::connect((host.as_ref(), port)));
        let ssl_stream = try!(SslStream::connect(&ssl_context, tcp_stream));

        debug!("Connection with {}:{} successfully established.", host, port);

        let ssl_stream_rc = Rc::new(RefCell::new(ssl_stream));

        let heartbeat = HeartbeatChannel::new(DEFAULT_SENDER_ID.to_owned(),
                                              DEFAULT_RECEIVER_ID.to_owned(),
                                              ssl_stream_rc.clone());
        let connection = ConnectionChannel::new(DEFAULT_SENDER_ID.to_owned(),
                                                ssl_stream_rc.clone());
        let receiver = ReceiverChannel::new(DEFAULT_SENDER_ID.to_owned(),
                                            DEFAULT_RECEIVER_ID.to_owned(),
                                            ssl_stream_rc.clone());
        let media = MediaChannel::new(DEFAULT_SENDER_ID.to_owned(), ssl_stream_rc.clone());

        Ok(Chromecast {
            stream: ssl_stream_rc,
            heartbeat: heartbeat,
            connection: connection,
            receiver: receiver,
            media: media,
        })
    }

    pub fn receive(&self) -> Result<ChannelMessage, Error> {
        let cast_message = try!(MessageManager::receive(&mut *self.stream.borrow_mut()));

        if self.connection.can_handle(&cast_message) {
            return Ok(ChannelMessage::Connection(try!(self.connection.parse(&cast_message))));
        }

        if self.heartbeat.can_handle(&cast_message) {
            return Ok(ChannelMessage::Hearbeat(try!(self.heartbeat.parse(&cast_message))));
        }

        if self.media.can_handle(&cast_message) {
            return Ok(ChannelMessage::Media(try!(self.media.parse(&cast_message))));
        }

        if self.receiver.can_handle(&cast_message) {
            return Ok(ChannelMessage::Receiver(try!(self.receiver.parse(&cast_message))));
        }

        Err(Error::Internal(
            format!("Unsupported message namespace: {}", cast_message.get_namespace())))
    }
}
