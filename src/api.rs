//! Contains nice wrapper around SC2 API.

use crate::client::{SC2Result, WS};
use protobuf::Message;
use sc2_proto::sc2api::{Request, Response};
use std::sync::{Mutex, MutexGuard};
use tungstenite::Message::Binary;

/// SC2 API. Can be accessed through [`ctx.api`](crate::Context::api).
pub struct API(Mutex<WS>);
impl API {
	pub(crate) fn new(ws: WS) -> API {
		API(Mutex::new(ws))
	}

	fn socket(&self) -> MutexGuard<'_, WS> {
		// A panic during a send leaves nothing the next request could trip over
		self.0.lock().unwrap_or_else(|e| e.into_inner())
	}

	/// Sends request and returns a response.
	pub fn send(&self, req: Request) -> SC2Result<Response> {
		let mut ws = self.socket();

		ws.write_message(Binary(req.write_to_bytes()?))?;

		let msg = ws.read_message()?;

		let mut res = Response::new();
		res.merge_from_bytes(msg.into_data().as_slice())?;
		Ok(res)
	}

	/// Sends request, waits for the response, but ignores it (useful when response is empty).
	pub fn send_request(&self, req: Request) -> SC2Result<()> {
		let mut ws = self.socket();
		ws.write_message(Binary(req.write_to_bytes()?))?;
		let _ = ws.read_message()?;
		Ok(())
	}

	/// Sends request, but doesn't wait for the response (use only when more control required,
	/// in common cases prefered to use [`send`] or [`send_request`]).
	///
	/// [`send`]: Self::send
	/// [`send_request`]: Self::send_request
	pub fn send_only(&self, req: Request) -> SC2Result<()> {
		self.socket().write_message(Binary(req.write_to_bytes()?))?;
		Ok(())
	}
	/// Waits for a response (useful only after [`send_only`]).
	///
	/// [`send_only`]: Self::send_only
	pub fn wait_response(&self) -> SC2Result<Response> {
		let msg = self.socket().read_message()?;

		let mut res = Response::new();
		res.merge_from_bytes(msg.into_data().as_slice())?;
		Ok(res)
	}
}
