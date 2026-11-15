use crate::{
    components::{Button, Icon},
    manager::Status,
    model::Instance,
    state::AppState,
    translator::{self, ApiError},
};
use dioxus::prelude::*;
use dioxus_core::spawn_forever;
use serde::Deserialize;
#[cfg(test)]
use std::io::Write;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

async fn choose_file(dialog: rfd::AsyncFileDialog) -> Option<PathBuf> {
    #[cfg(feature = "ui-test")]
    if let Some(path) = std::env::var_os("LIBRETRANSLATE_GUI_TEST_FILE") {
        return Some(path.into());
    }
    dialog
        .pick_file()
        .await
        .map(|file| file.path().to_path_buf())
}
async fn save_file_dialog(name: &str) -> Option<PathBuf> {
    #[cfg(feature = "ui-test")]
    if let Some(path) = std::env::var_os("LIBRETRANSLATE_GUI_TEST_SAVE_FILE") {
        return Some(path.into());
    }
    rfd::AsyncFileDialog::new()
        .set_file_name(name)
        .save_file()
        .await
        .map(|file| file.path().to_path_buf())
}

#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Formats {
    pub files_translation: bool,
    pub supported_files_format: Vec<String>,
}
pub fn formats(instance: &Instance) -> Result<Formats, ApiError> {
    translator::response(
        translator::client()?
            .get(format!(
                "{}/frontend/settings",
                instance.url().trim_end_matches('/')
            ))
            .send(),
    )
}
#[derive(Clone)]
pub struct OutputFile {
    pub name: String,
    data: Arc<StoredFile>,
}
// The OS removes this anonymous file when its last handle closes, including on
// process termination. Cloning a view shares only the handle, not the payload.
struct StoredFile {
    file: Mutex<File>,
    size: u64,
}
#[cfg(test)]
fn copy_download(input: &mut impl Read, output: &mut impl Write) -> Result<u64, ApiError> {
    let mut size = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer).map_err(translator::body_error)?;
        if count == 0 {
            return Ok(size);
        }
        output
            .write_all(&buffer[..count])
            .map_err(|_| ApiError::FileStorage)?;
        size += count as u64;
    }
}
impl OutputFile {
    pub fn save(&self, destination: &Path) -> std::io::Result<()> {
        let mut source = self
            .data
            .file
            .lock()
            .map_err(|_| std::io::Error::other("Translated file is unavailable"))?;
        source.seek(SeekFrom::Start(0))?;
        save_output_using(destination, |file| {
            let copied = std::io::copy(&mut *source, file)?;
            if copied != self.data.size {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Translated file is incomplete",
                ));
            }
            Ok(())
        })
    }
}
#[derive(Clone, Default)]
pub struct Draft {
    selected: Option<PathBuf>,
    result: Option<OutputFile>,
    pending: bool,
    choosing: bool,
    saving: bool,
    error: Option<&'static str>,
    generation: u64,
    languages: (String, String),
    transfer: Option<Arc<Transfer>>,
}
#[derive(Default)]
struct Transfer {
    cancel: AtomicBool,
    wake: tokio::sync::Notify,
    uploaded: AtomicU64,
    total: AtomicU64,
    downloaded: AtomicU64,
}
impl Transfer {
    fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.wake.notify_one();
    }
    async fn cancelled(&self) {
        if !self.cancel.load(Ordering::Relaxed) {
            self.wake.notified().await;
        }
    }
}
impl Draft {
    pub fn cancel(&self) {
        if let Some(transfer) = &self.transfer {
            transfer.cancel();
        }
    }
    fn languages(&mut self, source: String, target: String) {
        if self.languages != (source.clone(), target.clone()) {
            self.cancel();
            self.languages = (source, target);
            self.generation = self.generation.wrapping_add(1);
            self.result = None;
            self.error = None;
        }
    }
}
fn save_output_using(
    destination: &Path,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temporary = parent.join(format!(".libretranslate-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        write(&mut file)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, destination)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}
fn supported_extension(path: &Path, extensions: &[String]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            extensions
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(ext))
        })
}
pub fn download_url(instance: &Instance, value: &str) -> Result<reqwest::Url, ApiError> {
    let root = format!("{}/", instance.url().trim_end_matches('/'));
    let base = reqwest::Url::parse(&root).map_err(|_| ApiError::Invalid)?;
    let url = base.join(value).map_err(|_| ApiError::Invalid)?;
    let prefix = format!("{}download_file/", base.path());
    if url.origin() != base.origin()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.path().starts_with(&prefix)
        || url.path()[prefix.len()..].is_empty()
        || url.path()[prefix.len()..].contains('/')
    {
        return Err(ApiError::Invalid);
    }
    Ok(url)
}
#[cfg(test)]
fn upload_form(
    instance: &Instance,
    path: &Path,
    source: &str,
    target: &str,
) -> Result<reqwest::blocking::multipart::Form, ApiError> {
    let file = std::fs::File::open(path).map_err(|_| ApiError::Invalid)?;
    let metadata = file.metadata().map_err(|_| ApiError::Invalid)?;
    let size = metadata.len();
    if !metadata.is_file() || size == 0 {
        return Err(ApiError::Invalid);
    }
    // Read exactly the selected snapshot length, even if a log/document grows
    // while the multipart request is queued or being sent.
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or(ApiError::Invalid)?;
    // LibreTranslate's format check is case-sensitive, even on Windows.
    let filename = filename.rsplit_once('.').map_or_else(
        || filename.clone(),
        |(stem, extension)| format!("{stem}.{}", extension.to_ascii_lowercase()),
    );
    let part =
        reqwest::blocking::multipart::Part::reader_with_length(UploadReader(file.take(size)), size)
            .file_name(filename);
    let form = reqwest::blocking::multipart::Form::new()
        .part("file", part)
        .text("source", source.to_string())
        .text("target", target.to_string())
        .text("api_key", instance.server.api_key.clone());
    Ok(form)
}
// Take caps growth; a short read must also fail rather than letting multipart
// append its boundary to a file that was truncated after validation.
struct UploadReader(std::io::Take<File>);
impl Read for UploadReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.0.read(buffer)?;
        if count == 0 && !buffer.is_empty() && self.0.limit() != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "The selected file was truncated during upload",
            ));
        }
        Ok(count)
    }
}
#[cfg(test)]
fn download_file(
    response: Result<reqwest::blocking::Response, reqwest::Error>,
    directory: &Path,
) -> Result<Arc<StoredFile>, ApiError> {
    let mut response = response.map_err(|error| {
        if error.is_timeout() {
            ApiError::Timeout
        } else {
            ApiError::Connection
        }
    })?;
    if let Some(error) = translator::status_error(response.status()) {
        return Err(error);
    }
    // This endpoint is requested as a complete file without a Range header.
    // Empty/partial 2xx replies must not be offered as completed translations.
    if response.status() != reqwest::StatusCode::OK {
        return Err(ApiError::Invalid);
    }
    let expected_size = response.content_length();
    std::fs::create_dir_all(directory).map_err(|_| ApiError::FileStorage)?;
    let mut file = tempfile::tempfile_in(directory).map_err(|_| ApiError::FileStorage)?;
    let size = copy_download(&mut response, &mut file)?;
    if size == 0 || expected_size.is_some_and(|expected| expected != size) {
        return Err(ApiError::Invalid);
    }
    file.flush().map_err(|_| ApiError::FileStorage)?;
    Ok(Arc::new(StoredFile {
        file: Mutex::new(file),
        size,
    }))
}
#[cfg(test)]
pub fn translate_file(
    instance: &Instance,
    path: &Path,
    source: &str,
    target: &str,
    directory: &Path,
) -> Result<OutputFile, ApiError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| ApiError::Connection)?
        .block_on(translate_cancellable(
            instance,
            path,
            source,
            target,
            directory,
            Arc::new(Transfer::default()),
        ))
}

async fn translate_cancellable(
    instance: &Instance,
    path: &Path,
    source: &str,
    target: &str,
    directory: &Path,
    transfer: Arc<Transfer>,
) -> Result<OutputFile, ApiError> {
    tokio::select! {
        biased;
        _ = transfer.cancelled() => Err(ApiError::Cancelled),
        result = translate_async(instance, path, source, target, directory, transfer.clone()) => result,
    }
}

fn request_error(error: reqwest::Error) -> ApiError {
    if error.is_timeout() {
        ApiError::Timeout
    } else {
        ApiError::Connection
    }
}

async fn translate_async(
    instance: &Instance,
    path: &Path,
    source: &str,
    target: &str,
    directory: &Path,
    transfer: Arc<Transfer>,
) -> Result<OutputFile, ApiError> {
    use tokio::io::AsyncWriteExt;
    if !instance.server.file_translation {
        return Err(ApiError::Invalid);
    }
    let file = File::open(path).map_err(|_| ApiError::Invalid)?;
    let metadata = file.metadata().map_err(|_| ApiError::Invalid)?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(ApiError::Invalid);
    }
    let size = metadata.len();
    transfer.total.store(size, Ordering::Relaxed);
    let reader = UploadReader(file.take(size));
    let progress = transfer.clone();
    let stream =
        futures_util::stream::try_unfold((reader, progress), |(mut reader, progress)| async move {
            tokio::task::spawn_blocking(move || {
                if progress.cancel.load(Ordering::Relaxed) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "Cancelled",
                    ));
                }
                let mut buffer = vec![0; 128 * 1024];
                let count = reader.read(&mut buffer)?;
                if count == 0 {
                    return Ok(None);
                }
                buffer.truncate(count);
                progress.uploaded.fetch_add(count as u64, Ordering::Relaxed);
                Ok(Some((buffer, (reader, progress))))
            })
            .await
            .map_err(std::io::Error::other)?
        });
    let filename = path.file_name().ok_or(ApiError::Invalid)?.to_string_lossy();
    let filename = filename.rsplit_once('.').map_or_else(
        || filename.to_string(),
        |(stem, ext)| format!("{stem}.{}", ext.to_ascii_lowercase()),
    );
    let part =
        reqwest::multipart::Part::stream_with_length(reqwest::Body::wrap_stream(stream), size)
            .file_name(filename);
    let form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("source", source.to_string())
        .text("target", target.to_string())
        .text("api_key", instance.server.api_key.clone());
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(request_error)?;
    #[derive(Deserialize)]
    struct ResultFile {
        #[serde(rename = "translatedFileUrl")]
        url: String,
    }
    let mut response = client
        .post(format!(
            "{}/translate_file",
            instance.url().trim_end_matches('/')
        ))
        .timeout(std::time::Duration::from_secs(600))
        .multipart(form)
        .send()
        .await
        .map_err(request_error)?;
    if let Some(error) = translator::status_error(response.status()) {
        return Err(error);
    }
    let mut json = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(request_error)? {
        if json.len() + chunk.len() > 8 * 1024 * 1024 {
            return Err(ApiError::Invalid);
        }
        json.extend_from_slice(&chunk);
    }
    let reply: ResultFile = serde_json::from_slice(&json).map_err(|_| ApiError::Invalid)?;
    let url = download_url(instance, &reply.url)?;
    let mut response = client
        .get(url)
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await
        .map_err(request_error)?;
    if let Some(error) = translator::status_error(response.status()) {
        return Err(error);
    }
    if response.status() != reqwest::StatusCode::OK {
        return Err(ApiError::Invalid);
    }
    let expected = response.content_length();
    tokio::fs::create_dir_all(directory)
        .await
        .map_err(|_| ApiError::FileStorage)?;
    let temporary = tempfile::tempfile_in(directory).map_err(|_| ApiError::FileStorage)?;
    let mut file = tokio::fs::File::from_std(temporary);
    let mut size = 0;
    while let Some(chunk) = response.chunk().await.map_err(request_error)? {
        file.write_all(&chunk)
            .await
            .map_err(|_| ApiError::FileStorage)?;
        size += chunk.len() as u64;
        transfer.downloaded.store(size, Ordering::Relaxed);
    }
    if size == 0 || expected.is_some_and(|expected| expected != size) {
        return Err(ApiError::Invalid);
    }
    file.flush().await.map_err(|_| ApiError::FileStorage)?;
    let data = Arc::new(StoredFile {
        file: Mutex::new(file.into_std().await),
        size,
    });
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("translation");
    let extension = path
        .extension()
        .and_then(|name| name.to_str())
        .unwrap_or("txt");
    Ok(OutputFile {
        name: format!("{stem}.{target}.{extension}"),
        data,
    })
}

#[component]
pub fn FileTranslator(
    instance: ReadSignal<Instance>,
    status: ReadSignal<Status>,
    source: ReadSignal<String>,
    target: ReadSignal<String>,
) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let locale = state.read().locale;
    let id = instance().id;
    let mut transfer_progress = use_signal(|| (0u64, 0u64, 0u64));
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let progress = state
                .peek()
                .file_translations
                .get(&id)
                .and_then(|draft| draft.transfer.as_ref())
                .map(|transfer| {
                    (
                        transfer.uploaded.load(Ordering::Relaxed),
                        transfer.total.load(Ordering::Relaxed),
                        transfer.downloaded.load(Ordering::Relaxed),
                    )
                })
                .unwrap_or_default();
            if *transfer_progress.peek() != progress {
                transfer_progress.set(progress);
            }
        }
    });
    use_effect(move || {
        if status() != Status::Running {
            if let Some(draft) = state.peek().file_translations.get(&id) {
                draft.cancel();
            }
        }
    });
    use_hook(move || {
        state.write().file_translations.entry(id).or_default();
    });
    let formats = use_resource(move || {
        let instance = instance();
        let running = status() == Status::Running;
        async move {
            if !running {
                return Err(ApiError::Connection);
            }
            tokio::task::spawn_blocking(move || self::formats(&instance))
                .await
                .unwrap_or(Err(ApiError::Connection))
        }
    });
    use_effect(move || {
        let (source, target) = (source(), target());
        let different = state
            .peek()
            .file_translations
            .get(&id)
            .is_some_and(|draft| draft.languages != (source.clone(), target.clone()));
        if different {
            state
                .write()
                .file_translations
                .entry(id)
                .or_default()
                .languages(source, target);
        }
    });
    let draft = state
        .read()
        .file_translations
        .get(&id)
        .cloned()
        .unwrap_or_default();
    let info = formats
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .cloned();
    let allowed = status() == Status::Running
        && instance().server.file_translation
        && info.as_ref().is_some_and(|info| info.files_translation);
    let extensions = info
        .as_ref()
        .map(|info| {
            info.supported_files_format
                .iter()
                .map(|ext| ext.trim_start_matches('.').to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let formats_text = extensions
        .iter()
        .map(|ext| format!(".{ext}"))
        .collect::<Vec<_>>()
        .join(", ");
    let filename = draft
        .selected
        .as_ref()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned());
    let output = draft.result.clone();
    rsx! {section{class:"file-translator",
        if !instance().server.file_translation||info.as_ref().is_some_and(|info|!info.files_translation){p{class:"field-hint",{locale.text("translator_files_disabled")}}}
        else if status()==Status::Running&&info.is_none(){p{class:"field-hint",{locale.text(if formats.pending(){"translator_loading"}else{"translator_unavailable"})}}}
        div{class:"file-selection",Icon{kind:"file"}
            if let Some(filename)=filename {strong{dir:"auto","{filename}"}}else{p{{locale.text("translator_choose_file")}}}
            Button{class:"secondary",disabled:!allowed||draft.pending||draft.choosing,onclick:move|_|{
                {let mut state=state.write();let Some(draft)=state.file_translations.get_mut(&id)else{return};if draft.choosing{return};draft.choosing=true;}
                let extensions=extensions.clone();spawn_forever(async move{
                let mut dialog=rfd::AsyncFileDialog::new().set_title(locale.text("translator_choose_file"));
                if !extensions.is_empty(){dialog=dialog.add_filter(locale.text("translator_files"),&extensions);}
                if let Some(path)=choose_file(dialog).await {
                    let valid_size=path.metadata().is_ok_and(|metadata|metadata.is_file()&&metadata.len()>0);
                    let valid_extension=supported_extension(&path,&extensions);
                    let mut state=state.write();let Some(draft)=state.file_translations.get_mut(&id)else{return};
                    if !valid_size {draft.error=Some("translator_file_size");}else if !valid_extension {draft.error=Some("translator_file_format");}else{draft.selected=Some(path);draft.result=None;draft.error=None;draft.generation=draft.generation.wrapping_add(1);}
                }
                if let Some(draft)=state.write().file_translations.get_mut(&id){draft.choosing=false;}
            });},Icon{kind:"folder"}{locale.text("translator_choose_file")}}
        }
        if !formats_text.is_empty(){p{class:"field-hint file-formats",dir:"ltr","{formats_text}"}}
        div{class:"file-actions",
            Button{class:"primary",disabled:!allowed||draft.pending||draft.selected.is_none()||target().is_empty(),onclick:move|_|{
                let transfer=Arc::new(Transfer::default());
                let (path,revision)={let mut state=state.write();let Some(draft)=state.file_translations.get_mut(&id)else{return};if draft.pending{return};let Some(path)=draft.selected.clone()else{return};draft.pending=true;draft.transfer=Some(transfer.clone());draft.result=None;draft.error=None;(path,draft.generation)};
                let instance=instance();let source=source();let target=target();
                let directory=state.peek().active_paths.as_ref().map(|paths|paths.cache.join("translations")).unwrap_or_else(||std::env::temp_dir().join("libretranslate-translations"));
                let pid=state.peek().views.iter().find(|view|view.instance.id==id).and_then(|view|view.pid);
                spawn_forever(async move{let reply=translate_cancellable(&instance,&path,&source,&target,&directory,transfer).await;
                    let mut state=state.write();let current=state.views.iter().any(|view|view.instance==instance&&view.status==Status::Running&&view.pid==pid);
                    if let Some(draft)=state.file_translations.get_mut(&id){if revision==draft.generation&&current{match reply{Ok(file)=>draft.result=Some(file),Err(problem)=>draft.error=Some(problem.key())}}draft.pending=false;draft.transfer=None;}
                });
            },if draft.pending{span{class:"spinner",aria_hidden:"true"}}else{Icon{kind:"language"}}{locale.text(if draft.pending{"translator_working"}else{"translator_translate_file"})}}
            if draft.pending {Button{class:"secondary",onclick:move |_|{if let Some(draft)=state.peek().file_translations.get(&id){draft.cancel();}},{locale.text("cancel")}}}
            if let Some(file)=output {Button{class:"secondary",disabled:draft.saving,onclick:move|_|{let file=file.clone();
                {let mut state=state.write();let Some(draft)=state.file_translations.get_mut(&id)else{return};if draft.saving{return};draft.saving=true;}
                spawn_forever(async move{
                    if let Some(destination)=save_file_dialog(&file.name).await {
                        let saved=tokio::task::spawn_blocking(move||file.save(&destination)).await;
                        let mut state=state.write();
                        if matches!(saved,Ok(Ok(()))){state.notice("translator_file_saved");}else if let Some(draft)=state.file_translations.get_mut(&id){draft.error=Some("translator_file_save_failed");}
                    }
                    if let Some(draft)=state.write().file_translations.get_mut(&id){draft.saving=false;}
                });},Icon{kind:"download"}{locale.text("translator_save_file")}}}
        }
        if draft.pending {p{class:"field-hint",role:"status",{
            let (sent,total,received)=transfer_progress();
            if received>0 {locale.text("translator_download_progress").replace("{bytes}",&crate::models::format_bytes(received))}
            else if total>0&&sent<total {locale.text("translator_upload_progress").replace("{sent}",&crate::models::format_bytes(sent)).replace("{total}",&crate::models::format_bytes(total))}
            else {locale.text("translator_working").into()}
        }}}
        if let Some(key)=draft.error{p{class:"field-error",role:"status",{locale.text(key)}}}
    }}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        net::{TcpListener, TcpStream},
        thread::JoinHandle,
        time::{Duration, Instant},
    };

    type Handler = Box<dyn FnOnce(&mut TcpStream, u16) + Send>;
    fn stored(bytes: &[u8]) -> Arc<StoredFile> {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(bytes).unwrap();
        Arc::new(StoredFile {
            file: Mutex::new(file),
            size: bytes.len() as u64,
        })
    }
    fn output_bytes(output: &OutputFile) -> Vec<u8> {
        let mut file = output.data.file.lock().unwrap();
        file.rewind().unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        bytes
    }
    fn reply(stream: &mut TcpStream, status: &str, body: &[u8]) {
        let header = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes()).unwrap();
        let _ = stream.write_all(body);
    }
    fn fixture(handlers: Vec<Handler>) -> (Instance, JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let worker = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for handler in handlers {
                let until = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                Instant::now() < until,
                                "expected fixture request did not arrive"
                            );
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut chunk = [0; 4096];
                loop {
                    let count = stream.read(&mut chunk).unwrap();
                    assert!(count > 0, "incomplete fixture request");
                    request.extend_from_slice(&chunk[..count]);
                    if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]);
                        let size = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|value| value.trim().parse::<usize>().ok())
                            })
                            .unwrap_or_default();
                        if request.len() >= end + 4 + size {
                            break;
                        }
                    }
                }
                requests.push(String::from_utf8(request).unwrap());
                handler(&mut stream, port);
            }
            requests
        });
        let mut instance = Instance {
            port,
            ..Default::default()
        };
        instance.server.url_prefix = "/local".into();
        instance.server.api_key = "test-key".into();
        (instance, worker)
    }
    fn with_download(download: Handler) -> (Instance, JoinHandle<Vec<String>>) {
        fixture(vec![
            Box::new(|stream, port| {
                reply(stream,"200 OK",format!(r#"{{"translatedFileUrl":"http://127.0.0.1:{port}/local/download_file/result.txt"}}"#).as_bytes())
            }),
            download,
        ])
    }
    fn test_input(dir: &Path) -> PathBuf {
        let path = dir.join("input.txt");
        std::fs::write(&path, b"Original input").unwrap();
        path
    }
    #[test]
    fn cancelling_stalled_headers_or_download_closes_the_connection() {
        for downloading in [false, true] {
            let waiting = Arc::new(AtomicBool::new(false));
            let server_waiting = waiting.clone();
            let handler: Handler = Box::new(move |stream, _| {
                if downloading {
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\n\r\npartial")
                        .unwrap();
                    stream.flush().unwrap();
                }
                server_waiting.store(true, Ordering::Relaxed);
                let mut byte = [0];
                // Cancellation must close the socket, not merely hide the spinner.
                match stream.read(&mut byte) {
                    Ok(0) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
                    other => panic!("Request remained connected after cancellation: {other:?}"),
                }
            });
            let (instance, server) = if downloading {
                with_download(handler)
            } else {
                fixture(vec![handler])
            };
            let directory = tempfile::tempdir().unwrap();
            let path = test_input(directory.path());
            let transfer = Arc::new(Transfer::default());
            let cancel = transfer.clone();
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                tokio::spawn(async move {
                    while !waiting.load(Ordering::Relaxed)
                        || (downloading && cancel.downloaded.load(Ordering::Relaxed) == 0)
                    {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                    cancel.cancel();
                });
                let result = tokio::time::timeout(
                    Duration::from_secs(2),
                    translate_cancellable(
                        &instance,
                        &path,
                        "en",
                        "ro",
                        directory.path(),
                        transfer.clone(),
                    ),
                )
                .await
                .unwrap();
                assert!(matches!(result, Err(ApiError::Cancelled)));
                assert_eq!(
                    transfer.uploaded.load(Ordering::Relaxed),
                    std::fs::metadata(&path).unwrap().len()
                );
                // Keep polling the HTTP driver's runtime while it processes socket teardown.
                tokio::task::spawn_blocking(move || server.join().unwrap())
                    .await
                    .unwrap();
            });
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        }
    }
    #[test]
    fn audit_download_rejects_empty_and_partial_successes() {
        for status in ["200 OK", "204 No Content", "206 Partial Content"] {
            let (instance, worker) = with_download(Box::new(move |stream, _| {
                if status.starts_with("206") {
                    stream.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-6/20\r\nContent-Length: 7\r\nConnection: close\r\n\r\npartial").unwrap();
                } else {
                    reply(stream, status, b"");
                }
            }));
            let dir = tempfile::tempdir().unwrap();
            let path = test_input(dir.path());
            assert!(
                matches!(
                    translate_file(&instance, &path, "en", "ro", dir.path()),
                    Err(ApiError::Invalid)
                ),
                "accepted {status} as a complete translation"
            );
            assert_eq!(std::fs::read(&path).unwrap(), b"Original input");
            assert_eq!(worker.join().unwrap().len(), 2);
        }
    }
    #[test]
    fn audit_download_authorization_errors_remain_actionable() {
        for status in ["401 Unauthorized", "403 Forbidden"] {
            let (instance, worker) =
                with_download(Box::new(move |stream, _| reply(stream, status, b"denied")));
            let dir = tempfile::tempdir().unwrap();
            let path = test_input(dir.path());
            assert!(
                matches!(
                    translate_file(&instance, &path, "en", "ro", dir.path()),
                    Err(ApiError::Authorization)
                ),
                "lost authorization error from {status}"
            );
            worker.join().unwrap();
        }
    }
    #[test]
    fn malformed_upload_replies_never_trigger_a_download_or_modify_input() {
        for body in [
            "",
            "{",
            "{}",
            r#"{"translatedFileUrl":null}"#,
            r#"{"translatedFileUrl":"https://example.com/foreign"}"#,
            r#"{"translatedFileUrl":"/local/download_file/../secret"}"#,
        ] {
            let body = body.as_bytes().to_vec();
            let (instance, worker) = fixture(vec![Box::new(move |stream, _| {
                reply(stream, "200 OK", &body)
            })]);
            let dir = tempfile::tempdir().unwrap();
            let path = test_input(dir.path());
            assert!(matches!(
                translate_file(&instance, &path, "en", "ro", dir.path()),
                Err(ApiError::Invalid)
            ));
            assert_eq!(worker.join().unwrap().len(), 1);
            assert_eq!(std::fs::read(path).unwrap(), b"Original input");
        }
    }
    #[test]
    fn rejected_uploads_do_not_download_and_successful_binary_bytes_are_preserved() {
        for (status, expected_error) in [
            ("401 Unauthorized", ApiError::Authorization),
            ("403 Forbidden", ApiError::Authorization),
            ("413 Payload Too Large", ApiError::TooLarge),
            ("429 Too Many Requests", ApiError::RateLimited),
            ("503 Service Unavailable", ApiError::Server),
        ] {
            let (instance, worker) = fixture(vec![Box::new(move |stream, _| {
                reply(stream, status, b"not a JSON response")
            })]);
            let dir = tempfile::tempdir().unwrap();
            let path = test_input(dir.path());
            let result = translate_file(&instance, &path, "en", "ro", dir.path());
            assert!(matches!(result,Err(error) if error == expected_error));
            assert_eq!(worker.join().unwrap().len(), 1);
            assert_eq!(std::fs::read(path).unwrap(), b"Original input");
        }
        let bytes = vec![0xff, 0, 0x80, 0x25, 0x50, 0x44, 0x46];
        let expected = bytes.clone();
        let (instance, worker) =
            with_download(Box::new(move |stream, _| reply(stream, "200 OK", &bytes)));
        let dir = tempfile::tempdir().unwrap();
        let path = test_input(dir.path());
        let output = translate_file(&instance, &path, "en", "ro", dir.path()).unwrap();
        assert_eq!(output_bytes(&output), expected);
        let saved = dir.path().join("output.bin");
        output.save(&saved).unwrap();
        assert_eq!(std::fs::read(saved).unwrap(), expected);
        worker.join().unwrap();
    }
    #[test]
    fn truncated_downloads_are_rejected_and_temporary_storage_is_released() {
        let dir = tempfile::tempdir().unwrap();
        let path = test_input(dir.path());
        let (instance, worker) = with_download(Box::new(|stream, _| {
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\nConnection: close\r\n\r\nshort",
                )
                .unwrap();
        }));
        assert!(matches!(
            translate_file(&instance, &path, "en", "ro", dir.path()),
            Err(ApiError::Connection)
        ));
        worker.join().unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"Original input");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn downloads_beyond_the_old_limit_stream_to_disk_with_and_without_content_length() {
        for chunked in [false, true] {
            let (instance, worker) = with_download(Box::new(move |stream, _| {
                let size = 65 * 1024 * 1024;
                if chunked {
                    stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").unwrap();
                } else {
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n"
                    )
                    .unwrap();
                }
                let chunk = [b'x'; 64 * 1024];
                for _ in 0..size / chunk.len() {
                    if chunked {
                        write!(stream, "{:x}\r\n", chunk.len()).unwrap();
                    }
                    stream.write_all(&chunk).unwrap();
                    if chunked {
                        stream.write_all(b"\r\n").unwrap();
                    }
                }
                if chunked {
                    stream.write_all(b"0\r\n\r\n").unwrap();
                }
            }));
            let dir = tempfile::tempdir().unwrap();
            let path = test_input(dir.path());
            let output = translate_file(&instance, &path, "en", "ro", dir.path()).unwrap();
            assert_eq!(output.data.size, 65 * 1024 * 1024);
            let destination = dir.path().join("saved.txt");
            output.save(&destination).unwrap();
            let mut file = File::open(destination).unwrap();
            let mut buffer = [0; 64 * 1024];
            let mut read = 0;
            loop {
                let count = file.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                assert!(buffer[..count].iter().all(|byte| *byte == b'x'));
                read += count as u64;
            }
            assert_eq!(read, output.data.size);
            worker.join().unwrap();
            assert_eq!(std::fs::read(path).unwrap(), b"Original input");
            drop(output);
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        }
    }
    #[test]
    fn download_timeouts_preserve_timeout_error_in_headers_and_body() {
        let dir = tempfile::tempdir().unwrap();
        let (instance, worker) = fixture(vec![Box::new(|stream, _| {
            std::thread::sleep(Duration::from_millis(750));
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\nx");
        })]);
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(250))
            .build()
            .unwrap();
        assert!(matches!(
            download_file(
                client
                    .get(format!("{}/download_file/result.txt", instance.url()))
                    .send(),
                dir.path()
            ),
            Err(ApiError::Timeout)
        ));
        worker.join().unwrap();
        let (instance, worker) = fixture(vec![Box::new(|stream, _| {
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\n");
            std::thread::sleep(Duration::from_millis(750));
            let _ = stream.write_all(b"x");
        })]);
        assert!(matches!(
            download_file(
                client
                    .get(format!("{}/download_file/result.txt", instance.url()))
                    .send(),
                dir.path()
            ),
            Err(ApiError::Timeout)
        ));
        worker.join().unwrap();
    }
    #[test]
    fn audit_upload_does_not_include_bytes_appended_after_selection() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input.txt");
        std::fs::write(&path, b"original").unwrap();
        let form = upload_form(&Instance::default(), &path, "auto", "ro").unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"APPENDED-AFTER-SELECTION")
            .unwrap();
        let mut payload = Vec::new();
        form.into_reader().read_to_end(&mut payload).unwrap();
        assert!(!String::from_utf8_lossy(&payload).contains("APPENDED-AFTER-SELECTION"));
    }
    #[test]
    fn truncated_upload_is_rejected_instead_of_finishing_a_short_multipart_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input.txt");
        std::fs::write(&path, b"original document").unwrap();
        let form = upload_form(&Instance::default(), &path, "auto", "ro").unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(3)
            .unwrap();
        let error = form.into_reader().read_to_end(&mut Vec::new()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
    }
    #[test]
    fn empty_and_non_file_inputs_are_rejected_but_large_inputs_are_streamed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.txt");
        let file = std::fs::File::create(&path).unwrap();
        assert!(matches!(
            upload_form(&Instance::default(), &path, "auto", "ro"),
            Err(ApiError::Invalid)
        ));
        let size = 65 * 1024 * 1024;
        file.set_len(size).unwrap();
        let form = upload_form(&Instance::default(), &path, "auto", "ro").unwrap();
        let sent = std::io::copy(&mut form.into_reader(), &mut std::io::sink()).unwrap();
        assert!(sent > size && sent < size + 4096);
        assert!(matches!(
            upload_form(&Instance::default(), dir.path(), "auto", "ro"),
            Err(ApiError::Invalid)
        ));
    }
    #[test]
    fn file_formats_accept_uppercase_extensions() {
        let formats = vec!["txt".into(), "html".into()];
        assert!(supported_extension(Path::new("REPORT.TXT"), &formats));
        assert!(supported_extension(Path::new("page.HTML"), &formats));
        assert!(!supported_extension(Path::new("archive.zip"), &formats));
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("REPORT.TXT");
        std::fs::write(&path, b"Uppercase filename").unwrap();
        let mut form = upload_form(&Instance::default(), &path, "en", "ro")
            .unwrap()
            .into_reader();
        let mut payload = String::new();
        form.read_to_string(&mut payload).unwrap();
        assert!(payload.contains("filename=\"REPORT.txt\""));
        assert_eq!(std::fs::read(path).unwrap(), b"Uppercase filename");
    }
    #[test]
    fn stream_copy_bounds_reads_and_distinguishes_storage_failures() {
        struct Generated {
            remaining: usize,
            max_buffer: usize,
        }
        impl Read for Generated {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                self.max_buffer = self.max_buffer.max(buffer.len());
                let count = buffer.len().min(self.remaining);
                buffer[..count].fill(b'x');
                self.remaining -= count;
                Ok(count)
            }
        }
        struct FullDisk;
        impl Write for FullDisk {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("disk full"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let size = 129 * 1024 * 1024;
        let mut generated = Generated {
            remaining: size,
            max_buffer: 0,
        };
        assert_eq!(
            copy_download(&mut generated, &mut std::io::sink()).unwrap(),
            size as u64
        );
        assert!(generated.max_buffer <= 64 * 1024);
        assert_eq!(
            copy_download(&mut &b"Translation"[..], &mut FullDisk),
            Err(ApiError::FileStorage)
        );
    }
    #[test]
    fn disk_results_support_repeated_concurrent_saves_and_release_on_last_drop() {
        let (instance, worker) = with_download(Box::new(|stream, _| {
            reply(stream, "200 OK", b"Stored translation")
        }));
        let directory = tempfile::tempdir().unwrap();
        let path = test_input(directory.path());
        let cache = directory.path().join("translations");
        let output = translate_file(&instance, &path, "en", "ro", &cache).unwrap();
        worker.join().unwrap();
        let weak = Arc::downgrade(&output.data);
        let mut saves = Vec::new();
        for index in 0..2 {
            let output = output.clone();
            let destination = directory.path().join(format!("saved-{index}.txt"));
            saves.push(std::thread::spawn(move || {
                output.save(&destination).unwrap();
                destination
            }));
        }
        for save in saves {
            assert_eq!(
                std::fs::read(save.join().unwrap()).unwrap(),
                b"Stored translation"
            );
        }
        let retained = output.clone();
        drop(output);
        assert!(weak.upgrade().is_some());
        let destination = directory.path().join("saved-again.txt");
        retained.save(&destination).unwrap();
        assert_eq!(std::fs::read(destination).unwrap(), b"Stored translation");
        drop(retained);
        assert!(weak.upgrade().is_none());
        assert_eq!(std::fs::read_dir(cache).unwrap().count(), 0);
    }
    #[test]
    fn unusable_cache_is_reported_as_storage_error_without_changing_input() {
        let (instance, worker) = with_download(Box::new(|stream, _| {
            reply(stream, "200 OK", b"Translation")
        }));
        let directory = tempfile::tempdir().unwrap();
        let path = test_input(directory.path());
        let cache = directory.path().join("blocked-cache");
        std::fs::write(&cache, b"Preserve this file").unwrap();
        assert!(matches!(
            translate_file(&instance, &path, "en", "ro", &cache),
            Err(ApiError::FileStorage)
        ));
        worker.join().unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"Original input");
        assert_eq!(std::fs::read(cache).unwrap(), b"Preserve this file");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
    }
    #[test]
    fn rendering_shares_temporary_file_and_navigation_preserves_the_result() {
        let mut draft = Draft {
            result: Some(OutputFile {
                name: "large.ro.txt".into(),
                data: stored(b"Translated document"),
            }),
            languages: ("auto".into(), "ro".into()),
            ..Default::default()
        };
        let rendered = draft.clone();
        assert!(Arc::ptr_eq(
            &draft.result.as_ref().unwrap().data,
            &rendered.result.unwrap().data
        ));
        draft.languages("auto".into(), "ro".into());
        assert!(draft.result.is_some());
        draft.languages("auto".into(), "es".into());
        assert!(draft.result.is_none());
    }
    #[test]
    fn failed_file_save_preserves_existing_destination_and_cleans_temporary_file() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("translation.txt");
        std::fs::write(&path, "original").unwrap();
        let result = save_output_using(&path, |file| {
            file.write_all(b"partial")?;
            Err(std::io::Error::other("disk full"))
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        OutputFile {
            name: "output.txt".into(),
            data: stored("Bună".as_bytes()),
        }
        .save(&path)
        .unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "Bună");
    }
    #[test]
    fn failed_destination_replacement_preserves_directory_and_cleans_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("existing-folder");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("keep.txt"), b"keep").unwrap();
        assert!(
            OutputFile {
                name: "output.txt".into(),
                data: stored(b"new translation")
            }
            .save(&destination)
            .is_err()
        );
        assert_eq!(
            std::fs::read(destination.join("keep.txt")).unwrap(),
            b"keep"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn uploads_unicode_file_with_api_key_and_downloads_translated_bytes() {
        use std::{io::Write, net::TcpListener};
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("résumé.txt");
        std::fs::write(&input, "Hello, 世界").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(4)))
                    .unwrap();
                let mut request = Vec::new();
                let mut bytes = [0; 4096];
                loop {
                    let count = stream.read(&mut bytes).unwrap();
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&bytes[..count]);
                    if let Some(split) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..split]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|value| value.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if request.len() >= split + 4 + length {
                            break;
                        }
                    }
                }
                requests.push(String::from_utf8(request).unwrap());
                let body = if index == 0 {
                    format!(
                        r#"{{"translatedFileUrl":"http://127.0.0.1:{port}/local/download_file/result.txt"}}"#
                    )
                } else {
                    "Bună, 世界".into()
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
            requests
        });
        let mut instance = Instance {
            port,
            ..Default::default()
        };
        instance.server.url_prefix = "/local".into();
        instance.server.api_key = "test-key".into();
        let output = translate_file(&instance, &input, "auto", "ro", directory.path()).unwrap();
        assert_eq!(output.name, "résumé.ro.txt");
        assert_eq!(output_bytes(&output), "Bună, 世界".as_bytes());
        assert_eq!(std::fs::read_to_string(input).unwrap(), "Hello, 世界");
        let requests = server.join().unwrap();
        for expected in [
            "POST /local/translate_file",
            "multipart/form-data",
            "test-key",
            "Hello, 世界",
            "name=\"source\"",
            "name=\"target\"",
        ] {
            assert!(requests[0].contains(expected), "{expected}");
        }
        assert!(requests[1].starts_with("GET /local/download_file/result.txt"));
    }
    #[test]
    fn downloads_stay_in_the_instance_prefix() {
        let mut instance = Instance::default();
        instance.server.url_prefix = "/local".into();
        let valid = format!("{}/download_file/result.txt", instance.url());
        assert!(download_url(&instance, &valid).is_ok());
        for url in [
            "https://example.com/download_file/a",
            "http://127.0.0.1:5001/local/download_file/a",
            "/download_file/a",
            "/local/download_file/../secrets",
            "/local/download_file/a?token=x",
            "/local/download_file/",
        ] {
            assert!(download_url(&instance, url).is_err(), "{url}");
        }
    }
}
