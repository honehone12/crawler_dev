use serde_json::{json, Map};
use tokio::fs;
use anyhow::bail;
use tracing::{info, warn};
use http::Method;
use url::Url;
use bytes::Bytes;
use http_body_util::BodyExt;
use fantoccini::{elements::Element, ClientBuilder, Locator};
use clap::Parser;

#[derive(Parser)]
struct Target {
    #[arg(long)]
    url: String
}

async fn get_img(img: Element) -> anyhow::Result<(String, Bytes)> {
    let Some(src) = img.attr("src").await? else {
        bail!("could not find img src");
    };
    if src.starts_with("data:") {
        bail!("could not handle data url: {src}");
    }
    if !src.ends_with(".jpg") && !src.ends_with(".png") && !src.ends_with(".webp") {
        bail!("unsupported file: {src}");    
    }

    let raw = img.client().raw_client_for(Method::GET, &src).await?;
    let body = raw.into_body().collect().await?.to_bytes();
    Ok((src, body))
}

fn rename(name: &str) -> anyhow::Result<String> {
    if !name.ends_with(".jpg") && !name.ends_with(".png") && !name.ends_with(".webp") {
        bail!("unsupported file: {name}");
    }

    let Some(ex) = name.split('.').last() else {
        bail!("could not find file extension: {name}");
    };
    let hash = blake3::hash(name.as_bytes());
    let name = format!("{}.{ex}", hex::encode(hash.as_bytes()));
    Ok(name)
}

async fn get_video_src(iframe: Element) -> anyhow::Result<(String, String)> {
    let Some(src) = iframe.attr("src").await? else {
        bail!("could not find iframe src");            
    };
    if !src.starts_with("https://www.youtube.com/embed/") 
        && !src.starts_with("https://www.youtube-nocookie.com/embed/") 
    {
        bail!("could not find video src");
    }

    let url = Url::parse(&src)?;
    let Some(mut path) = url.path_segments() else {
        bail!("could not find path segments: {src}");
    };
    _ = path.next();
    let Some(id) = path.next() else {
        bail!("could not find video id segment");
    };

    Ok((src, id.to_string()))
}

async fn get_link(a: Element, current: &Url) -> anyhow::Result<String> {
    let Some(mut href) = a.attr("href").await? else {
        bail!("could not find a href");
    };
    if !href.starts_with("https:") && !href.starts_with("http:") {
        let url = current.join(&href)?;
        href = url.to_string()
    }

    Ok(href)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    let target = Target::parse();
    let target_url = Url::parse(&target.url)?;
    let Some(title) = target_url.domain() else {
        bail!("failed to get domain");
    };

    let mut cap = Map::new();
    cap.insert("moz:firefoxOptions".to_string(), json!({
        "args": ["-headless"],
        "log": json!({"level": "error"})
    }));

    let c = ClientBuilder::native()
        .capabilities(cap)
        .connect("htpp://localhost:4444").await?;
    c.set_ua("Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:141.0) Gecko/20100101 Firefox/141.0").await?;
 
    c.goto(&target.url).await?;
    c.wait().for_url(&target_url).await?;
    
    fs::create_dir(format!("output/{title}")).await?;
    
    {
        let mut list = vec![];
        let imgs = c.find_all(Locator::Css("img")).await?;
        for img in imgs {
            let (src, b) = match get_img(img).await {
                Ok(i) => i,
                Err(e) => {
                    warn!("{e}");
                    continue;
                }
            };
            let name = match rename(&src) {
                Ok(n) => n,
                Err(e) => {
                    warn!("{e}");
                    continue;
                }
            };
            fs::write(format!("output/{title}/{name}"), b).await?;

            list.push(json!({
                "src": src,
                "img": name
            }));
        }

        let json = serde_json::to_string_pretty(&list)?;
        fs::write(format!("output/{title}/images.json"), json).await?;
    }

    {
        let mut list = vec![];
        let iframes = c.find_all(Locator::Css("iframe")).await?;
        for iframe in iframes {
            let (src, id) = match get_video_src(iframe).await {
                Ok(v) => v,
                Err(e) => {
                    warn!("{e}");
                    continue;
                }
            };

            list.push(json!({
                "src": src,
                "id": id
            }));
        }

        let json = serde_json::to_string_pretty(&list)?;
        fs::write(format!("output/{title}/videos.json"), json).await?;
    }

    {
        let mut list = vec![];
        let links = c.find_all(Locator::Css("a")).await?;
        for link in links {
            let src = match get_link(link, &target_url).await {
                Ok(l) => l,
                Err(e) => {
                    warn!("{e}");
                    continue;
                }
            };

            list.push(src);
        }

        let json = serde_json::to_string_pretty(&list)?;
        fs::write(format!("output/{title}/links.json"), json).await?;
    }

    let html = c.source().await?;
    fs::write(format!("output/{title}/index.html"), html).await?;

    c.close().await?;
    info!("done");
    Ok(())
}
