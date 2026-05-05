use crate::{LocationLink, lsp_store::LspStore};
use anyhow::{Result, anyhow};
use gpui::{App, AsyncApp, Entity};
use rpc::proto;

pub trait LspCommand: 'static + Sized + Send + std::fmt::Debug {
    type Response: 'static + Default + Send + std::fmt::Debug;
    type LspRequest: 'static + Send + lsp::request::Request;

    fn display_name(&self) -> &str {
        "LSP request"
    }

    fn status(&self) -> Option<String> {
        None
    }

    fn check_capabilities(&self, _: lsp::AdapterServerCapabilities) -> bool {
        false
    }
}

pub fn location_link_from_proto(
    _: proto::LocationLink,
    _: Entity<LspStore>,
    _: &mut AsyncApp,
) -> gpui::Task<Result<LocationLink>> {
    gpui::Task::ready(Err(anyhow!(
        "LSP location links are unavailable in this build"
    )))
}

pub async fn location_links_from_proto(
    links: Vec<proto::LocationLink>,
    lsp_store: Entity<LspStore>,
    cx: &mut AsyncApp,
) -> Result<Vec<LocationLink>> {
    let mut locations = Vec::with_capacity(links.len());
    for link in links {
        locations.push(location_link_from_proto(link, lsp_store.clone(), cx).await?);
    }
    Ok(locations)
}

pub async fn location_link_from_lsp(
    _: lsp::LocationLink,
    _: Entity<LspStore>,
    _: lsp::LanguageServerId,
    _: &mut AsyncApp,
) -> Result<LocationLink> {
    Err(anyhow!("LSP location links are unavailable in this build"))
}

pub async fn location_links_from_lsp(
    links: Vec<lsp::LocationLink>,
    lsp_store: Entity<LspStore>,
    server_id: lsp::LanguageServerId,
    cx: &mut AsyncApp,
) -> Result<Vec<LocationLink>> {
    let mut locations = Vec::with_capacity(links.len());
    for link in links {
        locations.push(location_link_from_lsp(link, lsp_store.clone(), server_id, cx).await?);
    }
    Ok(locations)
}

pub fn location_link_to_proto(
    _: LocationLink,
    _: &mut LspStore,
    _: proto::PeerId,
    _: &mut App,
) -> proto::LocationLink {
    proto::LocationLink::default()
}

pub fn location_links_to_proto(
    links: Vec<LocationLink>,
    lsp_store: &mut LspStore,
    peer_id: proto::PeerId,
    cx: &mut App,
) -> Vec<proto::LocationLink> {
    links
        .into_iter()
        .map(|link| location_link_to_proto(link, lsp_store, peer_id, cx))
        .collect()
}
