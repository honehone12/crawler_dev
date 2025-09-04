use serde_json::json;
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
        bail!("could not handle data url");
    }
    if !src.ends_with(".jpg") && !src.ends_with(".png") && !src.ends_with(".webp") {
        bail!("unsupported file");    
    }

    let raw = img.client().raw_client_for(Method::GET, &src).await?;
    let body = raw.into_body().collect().await?.to_bytes();
    Ok((src, body))
}

fn rename(name: &str) -> anyhow::Result<String> {
    if !name.ends_with(".jpg") && !name.ends_with(".png") && !name.ends_with(".webp") {
        bail!("unsupported file");
    }

    let Some(ex) = name.split('.').last() else {
        bail!("could not find file extension");
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
        bail!("could not finc video src");
    }

    let url = Url::parse(&src)?;
    let Some(mut path) = url.path_segments() else {
        bail!("could not find path segments");
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

    let c = ClientBuilder::native().connect("htpp://localhost:4444").await?;
    c.set_ua("Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:141.0) Gecko/20100101 Firefox/141.0").await?;
 
    c.goto(&target.url).await?;
    c.wait().for_url(&Url::parse(&target.url)?).await?;
    let title = c.title().await?;
    fs::create_dir(&title).await?;

    let html = c.source().await?;
    let name = target.url.replace('/', "$");
    fs::write(format!("{title}/{name}.html"), html).await?;
    
    {
        let mut list = vec![];
        let imgs = c.find_all(Locator::Css("img")).await?;
        for img in imgs {
            let Ok((src, b)) = get_img(img).await else {
                warn!("failed to resolve image");
                continue;
            };
            let Ok(name) = rename(&src) else {
                warn!("failed to rename iamge");
                continue;
            };
            fs::write(format!("{title}/{name}"), b).await?;

            list.push(json!({
                "src": src,
                "img": name
            }));
        }

        let json = serde_json::to_string_pretty(&list)?;
        fs::write(format!("{title}/images.json"), json).await?;
    }

    {
        let mut list = vec![];
        let iframes = c.find_all(Locator::Css("iframe")).await?;
        for iframe in iframes {
            let Ok((src, id)) = get_video_src(iframe).await else {
                warn!("failed to resolve video src");
                continue;
            };

            list.push(json!({
                "src": src,
                "id": id
            }));
        }

        let json = serde_json::to_string_pretty(&list)?;
        fs::write(format!("{title}/videos.json"), json).await?;
    }

    {
        let mut list = vec![];
        let current = c.current_url().await?;
        let links = c.find_all(Locator::Css("a")).await?;
        for link in links {
            let Ok(src) = get_link(link, &current).await else {
                warn!("failed to resolve link");
                continue;
            };

            list.push(src);
        }

        let json = serde_json::to_string_pretty(&list)?;
        fs::write(format!("{title}/links.json"), json).await?;
    }

    c.close().await?;
    info!("done");
    Ok(())
}
