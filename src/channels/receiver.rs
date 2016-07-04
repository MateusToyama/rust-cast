use std::borrow::Cow;
use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;
use std::str::FromStr;
use std::string::ToString;
use serde_json::Value;
use serde_json::value::from_value;

use cast::cast_channel;
use errors::Error;
use message_manager::MessageManager;

const CHANNEL_NAMESPACE: &'static str = "urn:x-cast:com.google.cast.receiver";

const MESSAGE_TYPE_LAUNCH: &'static str = "LAUNCH";
const MESSAGE_TYPE_STOP: &'static str = "STOP";
const MESSAGE_TYPE_GET_STATUS: &'static str = "GET_STATUS";

const REPLY_TYPE_RECEIVER_STATUS: &'static str = "RECEIVER_STATUS";
const REPLY_TYPE_LAUNCH_ERROR: &'static str = "LAUNCH_ERROR";

const APP_DEFAULT_MEDIA_RECEIVER_ID: &'static str = "CC1AD845";
const APP_BACKDROP_ID: &'static str = "E8C28D3C";
const APP_YOUTUBE_ID: &'static str = "233637DE";

#[derive(Serialize, Debug)]
struct AppLaunchRequest {
    #[serde(rename="requestId")]
    pub request_id: i32,

    #[serde(rename="type")]
    pub typ: String,

    #[serde(rename="appId")]
    pub app_id: String,
}

#[derive(Serialize, Debug)]
struct AppStopRequest<'a> {
    #[serde(rename="requestId")]
    pub request_id: i32,

    #[serde(rename="type")]
    pub typ: String,

    #[serde(rename="sessionId")]
    pub session_id: Cow<'a, str>,
}

#[derive(Serialize, Debug)]
struct GetStatusRequest {
    #[serde(rename="requestId")]
    pub request_id: i32,

    #[serde(rename="type")]
    pub typ: String,
}

#[derive(Deserialize, Debug)]
pub struct StatusReply {
    #[serde(rename="requestId")]
    pub request_id: i32,

    #[serde(rename="type")]
    pub typ: String,

    pub status: ReceiverStatus,
}

#[derive(Deserialize, Debug)]
pub struct ReceiverStatus {
    #[serde(default)]
    pub applications: Vec<Application>,

    #[serde(rename="isActiveInput", default)]
    pub is_active_input: bool,

    #[serde(rename="isStandBy", default)]
    pub is_stand_by: bool,

    pub volume: ReceiverVolume,
}

#[derive(Deserialize, Debug)]
pub struct Application {
    #[serde(rename="appId")]
    pub app_id: String,

    #[serde(rename="sessionId")]
    pub session_id: String,

    #[serde(rename="transportId", default)]
    pub transport_id: String,

    #[serde(default)]
    pub namespaces: Vec<AppNamespace>,

    #[serde(rename="displayName")]
    pub display_name: String,

    #[serde(rename="statusText")]
    pub status_text: String,
}

#[derive(Deserialize, Debug)]
pub struct AppNamespace {
    pub name: String,
}

#[derive(Deserialize, Debug)]
pub struct ReceiverVolume {
    pub level: f64,
    pub muted: bool,
}

#[derive(Deserialize, Debug)]
pub struct LaunchErrorReply {
    #[serde(rename="type")]
    typ: String,
}

#[derive(Debug)]
pub enum ReceiverResponse {
    Status(StatusReply),
    LaunchError(LaunchErrorReply),
    Unknown,
}

#[derive(Debug, PartialEq)]
pub enum ChromecastApp {
    DefaultMediaReceiver,
    Backdrop,
    YouTube,
    Custom(String),
}

impl FromStr for ChromecastApp {
    type Err = ();

    fn from_str(s: &str) -> Result<ChromecastApp, ()> {
        let app = match s {
            APP_DEFAULT_MEDIA_RECEIVER_ID | "default" => ChromecastApp::DefaultMediaReceiver,
            APP_BACKDROP_ID | "backdrop" => ChromecastApp::Backdrop,
            APP_YOUTUBE_ID | "youtube" => ChromecastApp::YouTube,
            custom @ _ => ChromecastApp::Custom(custom.to_owned())
        };

        Ok(app)
    }
}

impl ToString for ChromecastApp {
    fn to_string(&self) -> String {
        match *self {
            ChromecastApp::DefaultMediaReceiver => APP_DEFAULT_MEDIA_RECEIVER_ID.to_owned(),
            ChromecastApp::Backdrop => APP_BACKDROP_ID.to_owned(),
            ChromecastApp::YouTube => APP_YOUTUBE_ID.to_owned(),
            ChromecastApp::Custom(ref app_id) => app_id.to_owned(),
        }
    }
}

pub struct ReceiverChannel<W> where W: Write {
    sender: String,
    receiver: String,
    writer: Rc<RefCell<W>>,
}

impl<W> ReceiverChannel<W> where W: Write {
    pub fn new(sender: String, receiver: String, writer: Rc<RefCell<W>>) -> ReceiverChannel<W> {
        ReceiverChannel {
            sender: sender,
            receiver: receiver,
            writer: writer,
        }
    }

    pub fn launch_app(&self, app: ChromecastApp) -> Result<(), Error> {
        let payload = AppLaunchRequest {
            typ: MESSAGE_TYPE_LAUNCH.to_owned(),
            request_id: 1,
            app_id: app.to_string(),
        };

        let message = try!(MessageManager::create(CHANNEL_NAMESPACE.to_owned(),
                                                  self.sender.clone(),
                                                  self.receiver.clone(),
                                                  Some(payload)));

        MessageManager::send(&mut *self.writer.borrow_mut(), message)
    }

    pub fn stop_app<'a, S>(&self, session_id: S) -> Result<(), Error> where S: Into<Cow<'a, str>> {
        let payload = AppStopRequest {
            typ: MESSAGE_TYPE_STOP.to_owned(),
            request_id: 1,
            session_id: session_id.into(),
        };

        let message = try!(MessageManager::create(CHANNEL_NAMESPACE.to_owned(),
                                                  self.sender.clone(),
                                                  self.receiver.clone(),
                                                  Some(payload)));

        MessageManager::send(&mut *self.writer.borrow_mut(), message)
    }

    pub fn get_status(&self) -> Result<(), Error> {
        let payload = GetStatusRequest {
            typ: MESSAGE_TYPE_GET_STATUS.to_owned(),
            request_id: 1,
        };

        let message = try!(MessageManager::create(CHANNEL_NAMESPACE.to_owned(),
                                                  self.sender.clone(),
                                                  self.receiver.clone(),
                                                  Some(payload)));

        MessageManager::send(&mut *self.writer.borrow_mut(), message)
    }

    pub fn can_handle(&self, message: &cast_channel::CastMessage) -> bool {
        message.get_namespace() == CHANNEL_NAMESPACE
    }

    pub fn parse(&self, message: &cast_channel::CastMessage) -> Result<ReceiverResponse, Error> {
        let reply: Value = try!(MessageManager::parse_payload(message));

        let message_type = match reply.as_object()
            .and_then(|object| object.get("type"))
            .and_then(|property| property.as_string()) {
            None => return Err(Error::Internal("Unexpected reply format".to_owned())),
            Some(string) => string.to_owned()
        };

        let reply = match &message_type as &str {
            REPLY_TYPE_RECEIVER_STATUS => ReceiverResponse::Status(try!(from_value(reply))),
            REPLY_TYPE_LAUNCH_ERROR => ReceiverResponse::LaunchError(try!(from_value(reply))),
            _ => ReceiverResponse::Unknown,
        };

        Ok(reply)
    }
}
