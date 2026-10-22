//! Text completion seam. Acoustic workers never own provider discovery or keys.
use crate::driver::RouteError;
use async_trait::async_trait;
use nomifun_api_types::voice::{VoiceDialogueRequest, VoiceDialogueResponse};

#[async_trait]
pub trait VoiceCompletionClient: Send + Sync {
    async fn complete(&self, request: &VoiceDialogueRequest) -> Result<String, RouteError>;
    async fn complete_stream(&self,request:&VoiceDialogueRequest,_sentences:tokio::sync::mpsc::Sender<nomifun_api_types::voice::VoiceDialogueSentence>)->Result<String,RouteError> {self.complete(request).await}
}

pub struct HostDialogueClient {
    endpoint: String,
    credential: String,
    client: reqwest::Client,
}

impl HostDialogueClient {
    pub fn new(endpoint: String, credential: String) -> Result<Self, RouteError> {
        let url = reqwest::Url::parse(&endpoint)
            .map_err(|_| RouteError::Generation("invalid_host_dialogue_endpoint".into()))?;
        if url.scheme() != "http"
            || !matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"))
            || credential.is_empty()
        {
            return Err(RouteError::Generation(
                "invalid_host_dialogue_endpoint".into(),
            ));
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(35))
            .build()
            .map_err(|_| RouteError::Network("host_dialogue_client_failed".into()))?;
        Ok(Self {
            endpoint,
            credential,
            client,
        })
    }
}

impl HostDialogueClient {
    async fn request(&self,request:&VoiceDialogueRequest)->Result<reqwest::Response,RouteError> {
        let response=self.client.post(&self.endpoint).header("x-ai-mate-control-token",&self.credential).json(request).send().await
            .map_err(|_|RouteError::Network("host_dialogue_unreachable".into()))?;
        if !response.status().is_success() {
            let code=response.json::<serde_json::Value>().await.ok().and_then(|v|v["code"].as_str().map(str::to_owned)).unwrap_or_else(||"model_rejected".into());
            return Err(RouteError::Generation(code));
        }
        Ok(response)
    }
    fn finish(response:VoiceDialogueResponse)->Result<String,RouteError> {
        match response.record_error {Some(code)=>Err(RouteError::Record{code,content:response.content}),None=>Ok(response.content)}
    }
}
#[async_trait]
impl VoiceCompletionClient for HostDialogueClient {
    async fn complete(&self,request:&VoiceDialogueRequest)->Result<String,RouteError> {
        Self::finish(self.request(request).await?.json::<VoiceDialogueResponse>().await
            .map_err(|_|RouteError::Generation("model_response_invalid".into()))?)
    }
    async fn complete_stream(&self,request:&VoiceDialogueRequest,sentences:tokio::sync::mpsc::Sender<nomifun_api_types::voice::VoiceDialogueSentence>)->Result<String,RouteError> {
        use nomifun_api_types::voice::{VoiceDialogueFrame,VoiceIntentType,parse_voice_decision};
        let mut request=request.clone();request.stream=true;
        let mut response=self.request(&request).await?;
        if !response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v|v.to_str().ok()).is_some_and(|v|v.starts_with("application/x-ndjson")) {
            return Self::finish(response.json::<VoiceDialogueResponse>().await.map_err(|_|RouteError::Generation("model_response_invalid".into()))?);
        }
        let invalid=||RouteError::Generation("model_response_invalid".into());
        let mut buffer=Vec::new();let mut total=0;let mut spoken=String::new();let mut count=0;
        while let Some(chunk)=response.chunk().await.map_err(|_|RouteError::Network("host_dialogue_unreachable".into()))? {
            total+=chunk.len();if total>65_536{return Err(invalid());}buffer.extend_from_slice(&chunk);
            while let Some(end)=buffer.iter().position(|b|*b==b'\n') {
                let frame:VoiceDialogueFrame=serde_json::from_slice(&buffer[..end]).map_err(|_|invalid())?;
                buffer.drain(..=end);
                match frame {
                    VoiceDialogueFrame::Sentence{sentence}=>{
                        count+=1;if count>4||sentence.text.trim().is_empty()||spoken.chars().count()+sentence.text.chars().count()>60{return Err(invalid());}
                        spoken.push_str(&sentence.text);
                        sentences.send(sentence).await.map_err(|_|RouteError::Network("voice_stream_closed".into()))?;
                    },
                    VoiceDialogueFrame::Complete{response}=>{
                        if !buffer.iter().all(u8::is_ascii_whitespace){return Err(invalid());}
                        if count>0 {
                            let decision=parse_voice_decision(&response.content).map_err(|_|invalid())?;
                            if decision.intent_type!=VoiceIntentType::Chat||decision.reply_tts.as_deref()!=Some(spoken.as_str()){return Err(invalid());}
                        }
                        return Self::finish(response);
                    },
                    VoiceDialogueFrame::Error{code}=>return Err(RouteError::Generation(code)),
                }
            }
        }
        Err(invalid())
    }
}
