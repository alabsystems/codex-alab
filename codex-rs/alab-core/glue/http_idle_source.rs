// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! An idle reconnect owner observes frozen authority without retaining source activity.
//! The application permit keeps upstream's sticky revocation semantics; a real source
//! permit is admitted only while the bounded recovery episode is being polled.

use super::*;

pub struct IdleNetworkPermit {
    policy: NetworkPolicy,
    url: Url,
    application: NetworkPermit,
}

impl fmt::Debug for IdleNetworkPermit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IdleNetworkPermit")
            .finish_non_exhaustive()
    }
}

impl NetworkPolicy {
    pub fn alab_idle_permit(&self, url: &Url) -> Result<IdleNetworkPermit, NetworkPolicyDenied> {
        let mut application = self.acquire(url)?;
        // Validate the original source at capture, but retain only its non-counting
        // policy below. The application observer must not hold an idle resource.
        drop(application.alab_native.take());
        Ok(IdleNetworkPermit {
            policy: self.clone(),
            url: url.clone(),
            application,
        })
    }
}

impl IdleNetworkPermit {
    pub fn check(&self) -> Result<(), NetworkPolicyDenied> {
        self.application.check()?;
        // The original source must still admit work. This temporary observation
        // owns no activity after return and never loads a newer source.
        self.policy.acquire(&self.url)?.check()
    }

    pub async fn run<T>(
        &self,
        operation: impl Future<Output = T>,
    ) -> Result<T, NetworkPolicyDenied> {
        self.application
            .run(async {
                let episode = self.policy.acquire(&self.url)?;
                episode.run(operation).await
            })
            .await?
    }
}

#[cfg(test)]
#[path = "http_idle_source_tests.rs"]
mod tests;
