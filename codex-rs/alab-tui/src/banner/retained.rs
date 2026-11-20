// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Native historical banner values; measured text is DATA, not a current harness/proof grant.
use super::*;
use serde::Deserialize;
use serde::Serialize;
use std::borrow::Cow;
use std::io;
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HistoricalClaim {
    pub(super) measure: claims::Measure,
    pub(super) alab: u32,
    pub(super) upstream: u32,
    pub(super) against: Cow<'static, str>,
    pub(super) measured: Cow<'static, str>,
}
impl HistoricalClaim {
    pub(super) fn borrowed(actual: &'static claims::Claim) -> Self {
        Self {
            measure: actual.measure,
            alab: actual.alab,
            upstream: actual.upstream,
            against: Cow::Borrowed(actual.against),
            measured: Cow::Borrowed(actual.measured),
        }
    }
    pub(super) fn forms(&self) -> Vec<Vec<claims::Run>> {
        claims::forms(self.measure, self.alab, self.upstream, &self.against)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BannerData {
    style: u8,
    depth: u8,
    background: Option<u32>,
    version: Option<String>,
    upstream: Option<String>,
    claims: Vec<ClaimData>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClaimData {
    measure: u8,
    alab: u32,
    upstream: u32,
    against: String,
    measured: String,
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
impl BannerData {
    pub fn capture(actual: &Banner) -> Self {
        Self {
            style: match actual.style {
                Style::Ribbon => 0,
                Style::Codex => 1,
                Style::Stripes => 2,
            },
            depth: match actual.depth {
                Depth::TrueColor => 0,
                Depth::Ansi256 => 1,
                Depth::Ansi16 => 2,
                Depth::Mono => 3,
            },
            background: actual.theme.bg,
            version: actual.version.clone(),
            upstream: actual.upstream.clone(),
            claims: actual
                .claims
                .iter()
                .map(|claim| ClaimData {
                    measure: match claim.measure {
                        claims::Measure::FootprintMb => 0,
                        claims::Measure::Daemons => 1,
                        claims::Measure::IdleConnections => 2,
                    },
                    alab: claim.alab,
                    upstream: claim.upstream,
                    against: claim.against.to_string(),
                    measured: claim.measured.to_string(),
                })
                .collect(),
        }
    }
    pub fn restore(self) -> io::Result<Banner> {
        if self.background.is_some_and(|color| color > 0x00ff_ffff) || self.claims.len() > 1_000_000
        {
            return Err(invalid("historical banner background/claim capacity"));
        }
        let style = match self.style {
            0 => Style::Ribbon,
            1 => Style::Codex,
            2 => Style::Stripes,
            _ => return Err(invalid("unknown original banner style")),
        };
        let depth = match self.depth {
            0 => Depth::TrueColor,
            1 => Depth::Ansi256,
            2 => Depth::Ansi16,
            3 => Depth::Mono,
            _ => return Err(invalid("unknown original banner depth")),
        };
        let claims = self
            .claims
            .into_iter()
            .map(|claim| {
                Ok(HistoricalClaim {
                    measure: match claim.measure {
                        0 => claims::Measure::FootprintMb,
                        1 => claims::Measure::Daemons,
                        2 => claims::Measure::IdleConnections,
                        _ => return Err(invalid("unknown original banner measure")),
                    },
                    alab: claim.alab,
                    upstream: claim.upstream,
                    against: Cow::Owned(claim.against),
                    measured: Cow::Owned(claim.measured),
                })
            })
            .collect::<io::Result<_>>()?;
        Ok(Banner {
            style,
            depth,
            theme: Theme {
                bg: self.background,
                ..Theme::default()
            },
            version: self.version,
            upstream: self.upstream,
            claims,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_unreported_background_omits_the_hosts_foreground_policy() -> io::Result<()> {
        let historical = Banner::new(Style::Codex, Depth::TrueColor, Theme::default(), "0.11")
            .with_upstream(Some("0.169".into()));
        let local = Banner {
            theme: historical.theme.with_terminal_foreground_if_unreported(),
            ..historical.clone()
        };
        let encoded = serde_json::to_vec(&BannerData::capture(&local))?;
        assert_eq!(
            encoded,
            serde_json::to_vec(&BannerData::capture(&historical))?
        );
        let restored = serde_json::from_slice::<BannerData>(&encoded)?.restore()?;
        assert_eq!(restored, historical);
        Ok(())
    }

    #[test]
    fn actual_banner_fields_and_measure_reducer_survive_owned_historical_restore() -> io::Result<()>
    {
        let original = Banner::new(
            Style::Codex,
            Depth::Ansi256,
            Theme::from_bg(Some((17, 29, 43))),
            "historical-native-build",
        )
        .with_upstream(Some("original-upstream".into()))
        .with_claims(claims::CLAIMS);
        let restored = serde_json::from_slice::<BannerData>(
            &serde_json::to_vec(&BannerData::capture(&original))
                .map_err(|e| io::Error::other(e.to_string()))?,
        )
        .map_err(|e| io::Error::other(e.to_string()))?
        .restore()?;
        assert_eq!(restored, original);
        for (a, b) in original.claims.iter().zip(&restored.claims) {
            assert_eq!(a.forms(), b.forms());
            assert!(matches!(&b.against, Cow::Owned(_)));
        }
        for width in [0, 10, 80, 160] {
            assert_eq!(
                original.lines_with_harness(width, None),
                restored.lines_with_harness(width, None)
            );
        }
        Ok(())
    }
}
