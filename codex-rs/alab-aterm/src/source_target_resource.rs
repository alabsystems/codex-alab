// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Resource admission uses its actual member channel; ACK retains exact receipt
//! correlation on a separate authenticated control channel across uncertain I/O.

use super::*;
use codex_alab_core::assignment::Binding;
use codex_alab_core::assignment::Generations;

frame!(ResourceFrame, String, registration: String, resource_receipt: String);
frame!(AppliedFrame, serde_json::Value,
    phase: String, boundary: String, assignment_generation: u64,
    credential_generation: u64, digest: String, account: String, user: String,
    auth_generation: u64, owner_generation: u64, source_generation: u64, login_generation: u64);

impl NativeMember {
    pub(in crate::source_member) async fn target_resource_ready_context(
        &self,
        operation: &str,
        report: &native_target::ResourceReport,
    ) -> io::Result<native_target::ResourceReceipt> {
        let role = self.target.as_ref().ok_or_else(unavailable)?;
        report.validate_context(&role.context)?;
        if report.member() != self.id
            || self.retired.load(std::sync::atomic::Ordering::Acquire)
            || !matches!(&*self.kind.lock().map_err(|_| unavailable())?,
                Kind::Managed(scope) if scope.as_ref() == &Scope::from(report.source()))
        {
            return Err(unavailable());
        }
        let binding = report.binding();
        let fields = format!(
            " member={}{} registration={} boundary={} assignment_generation={} credential_generation={} digest={} account={} user={} auth_generation={} owner_generation={} source_generation={} login_generation={}",
            self.id,
            role.fields(),
            report.registration(),
            report.boundary(),
            binding.assignment_generation,
            binding.credential_generation,
            binding.digest,
            binding.account,
            binding.user,
            binding.generations.auth,
            binding.generations.owner,
            binding.generations.source,
            binding.generations.login,
        );
        let body = if self.transport.conn.is_open() {
            self.transport
                .call(
                    "member-target-resource-ready",
                    operation,
                    &fields,
                    Some(&self.id),
                )
                .await?
        } else {
            // Correlation-only repair. Native may return an already issued
            // exact receipt, but this new channel cannot admit resource work.
            let transport =
                connect_transport(&self.transport.endpoint, &self.transport.birth).await?;
            transport.bind_epoch(&self.transport.epoch()?.ok_or_else(unavailable)?)?;
            let result = transport
                .call(
                    "member-target-resource-ready",
                    operation,
                    &fields,
                    Some(&self.id),
                )
                .await;
            transport.conn.close();
            result?
        };
        let result = (|| {
            let frame: ResourceFrame = serde_json::from_str(&body).map_err(|_| unavailable())?;
            frame.validate(
                &frame.request,
                operation,
                &self.transport.sid,
                &self.transport.birth,
                self.transport.epoch()?.as_deref(),
            )?;
            if frame.member != self.id || frame.registration != report.registration() {
                return Err(unavailable());
            }
            native_target::ResourceReceipt::from_native(
                &role.context,
                operation.into(),
                frame.resource_receipt,
                report.clone(),
            )
        })();
        self.fail(result)
    }
}

impl Target {
    pub(super) async fn acknowledge_receipt(
        &self,
        receipt: &native_target::ResourceReceipt,
    ) -> io::Result<native_target::Applied> {
        receipt.report().validate_context(&self.role.context)?;
        let parent = &self.role.transport;
        let transport = connect_transport(&parent.endpoint, &parent.birth).await?;
        transport.bind_epoch(&parent.epoch()?.ok_or_else(unavailable)?)?;
        let result = async {
            let fields = format!(
                " context={} resource_receipt={}",
                self.role.context.private_capability(),
                receipt.private_capability()
            );
            let operation = self.role.context.assignment();
            let body = transport
                .call("member-target-ack", operation, &fields, None)
                .await?;
            let frame: AppliedFrame = serde_json::from_str(&body).map_err(|_| unavailable())?;
            frame.validate(
                &frame.request,
                operation,
                &transport.sid,
                &transport.birth,
                transport.epoch()?.as_deref(),
            )?;
            if !frame.member.is_null() || frame.phase != "applied" {
                return Err(unavailable());
            }
            let observed = Binding {
                account: frame.account,
                user: frame.user,
                digest: frame.digest,
                credential_generation: frame.credential_generation,
                assignment_generation: frame.assignment_generation,
                generations: Generations {
                    auth: frame.auth_generation,
                    owner: frame.owner_generation,
                    source: frame.source_generation,
                    login: frame.login_generation,
                },
            };
            native_target::Applied::from_native(receipt, &frame.boundary, &observed)
        }
        .await;
        transport.conn.close();
        result
    }
}
