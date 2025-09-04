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

    let raw = img.client().raw_client_for(Method::GET, &src).await?;
    let body = raw.into_body().collect().await?.to_bytes();
    Ok((src, body))
}

async fn get_video_src(iframe: Element) -> anyhow::Result<(String, String)> {
    let Some(src) = iframe.attr("src").await? else {
        bail!("could not find iframe src");            
    };
    if !src.starts_with("https://www.youtube.com/embed/") {
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
        
        let imgs = c.find_all(Locator::Css("img")).await?;
        for img in imgs {
            let Ok((s, b)) = get_img(img).await else {
                warn!("failed to resolve image");
                continue;
            };
            let name = s.replace('/', "$");
            fs::write(format!("{title}/{name}"), b).await?;
        }
    }

    {
        let iframes = c.find_all(Locator::Css("iframe")).await?;
        for iframe in iframes {
            let Ok((s, id)) = get_video_src(iframe).await else {
                warn!("failed to resolve video src");
                continue;
            };
        }
    }


    c.close().await?;
    info!("ok");
    Ok(())
}
