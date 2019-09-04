// Copyright 2019. The Tari Project
//
// Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
// following conditions are met:
//
// 1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
// disclaimer.
//
// 2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
// following disclaimer in the documentation and/or other materials provided with the distribution.
//
// 3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
// products derived from this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
// INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
// WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
// USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
use crate::{message::InboundMessage, peer_manager::PeerNodeIdentity, types::CommsPublicKey};
use derive_error::Error;
use futures::{executor::block_on, stream::FusedStream, Stream, StreamExt};
use std::fmt::Debug;
use tari_utilities::message_format::MessageFormat;

#[derive(Debug, Error, PartialEq)]
pub enum DomainSubscriberError {
    /// Subscription stream ended
    SubscriptionStreamEnded,
    /// Error reading from the stream
    StreamError,
    /// Message deserialization error
    MessageError,
    /// Subscription Reader is not initialized
    SubscriptionReaderNotInitialized,
}

/// Information about the message received
#[derive(Debug, Clone)]
pub struct MessageInfo {
    pub peer_source: PeerNodeIdentity,
    pub origin_source: CommsPublicKey,
}
pub struct SyncDomainSubscription<S> {
    subscription: Option<S>,
}
impl<S> SyncDomainSubscription<S>
where S: Stream<Item = InboundMessage> + Unpin + FusedStream
{
    pub fn new(stream: S) -> Self {
        SyncDomainSubscription {
            subscription: Some(stream),
        }
    }

    pub fn receive_messages<T>(&mut self) -> Result<Vec<(MessageInfo, T)>, DomainSubscriberError>
    where T: MessageFormat {
        let subscription = self.subscription.take();

        match subscription {
            Some(mut s) => {
                let (stream_messages, stream_complete): (Vec<InboundMessage>, bool) = block_on(async {
                    let mut result = Vec::new();
                    let mut complete = false;
                    loop {
                        futures::select!(
                            item = s.next() => {
                                if let Some(item) = item {
                                    result.push(item)
                                }
                            },
                            complete => {
                                complete = true;
                                break
                            },
                            default => break,
                        );
                    }
                    (result, complete)
                });

                let mut messages = Vec::new();

                for m in stream_messages {
                    messages.push((
                        MessageInfo {
                            peer_source: m.peer_source,
                            origin_source: m.origin_source,
                        },
                        m.message
                            .deserialize_message()
                            .map_err(|_| DomainSubscriberError::MessageError)?,
                    ));
                }

                if !stream_complete {
                    self.subscription = Some(s);
                }

                return Ok(messages);
            },
            None => return Err(DomainSubscriberError::SubscriptionStreamEnded),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        message::Message,
        peer_manager::NodeIdentity,
        pub_sub_channel::{pubsub_channel, TopicPayload},
    };
    use futures::{executor::block_on, SinkExt};
    use serde::{Deserialize, Serialize};
    #[test]
    fn topic_pub_sub() {
        let (mut publisher, subscriber_factory) = pubsub_channel(10);

        #[derive(Serialize, Deserialize, Debug, Clone)]
        struct Dummy {
            a: u32,
            b: String,
        }

        let node_id = NodeIdentity::random_for_test(None);

        let messages = vec![
            ("Topic1".to_string(), Dummy {
                a: 1u32,
                b: "one".to_string(),
            }),
            ("Topic2".to_string(), Dummy {
                a: 2u32,
                b: "two".to_string(),
            }),
            ("Topic1".to_string(), Dummy {
                a: 3u32,
                b: "three".to_string(),
            }),
            ("Topic2".to_string(), Dummy {
                a: 4u32,
                b: "four".to_string(),
            }),
            ("Topic1".to_string(), Dummy {
                a: 5u32,
                b: "five".to_string(),
            }),
            ("Topic2".to_string(), Dummy {
                a: 6u32,
                b: "size".to_string(),
            }),
            ("Topic1".to_string(), Dummy {
                a: 7u32,
                b: "seven".to_string(),
            }),
        ];

        let serialized_messages = messages.iter().map(|m| {
            TopicPayload::new(
                m.0.clone(),
                InboundMessage::new(
                    node_id.identity.clone(),
                    node_id.identity.public_key.clone(),
                    Message::from_message_format(m.0.clone(), m.1.clone()).unwrap(),
                ),
            )
        });

        block_on(async {
            for m in serialized_messages {
                publisher.send(m).await.unwrap();
            }
        });
        drop(publisher);

        let mut domain_sub =
            SyncDomainSubscription::new(subscriber_factory.get_subscription("Topic1".to_string()).fuse());

        let messages = domain_sub.receive_messages::<Dummy>().unwrap();

        assert_eq!(
            domain_sub.receive_messages::<Dummy>().unwrap_err(),
            DomainSubscriberError::SubscriptionStreamEnded
        );

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].1.a, 1);
        assert_eq!(messages[1].1.a, 3);
        assert_eq!(messages[2].1.a, 5);
        assert_eq!(messages[3].1.a, 7);
    }
}
