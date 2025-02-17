// Copyright 2021 Parity Technologies (UK) Ltd.
// This file is part of Cumulus.

// Cumulus is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Cumulus is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Cumulus.  If not, see <http://www.gnu.org/licenses/>.

//! Cumulus CLI library.

#![warn(missing_docs)]

use clap::Parser;
use sc_service::{
	config::{PrometheusConfig, TelemetryEndpoints},
	BasePath, TransactionPoolOptions,
};
use std::{
	fs,
	io::{self, Write},
	net::SocketAddr,
};
use url::Url;

/// The `purge-chain` command used to remove the whole chain: the parachain and the relay chain.
#[derive(Debug, Parser)]
pub struct PurgeChainCmd {
	/// The base struct of the purge-chain command.
	#[clap(flatten)]
	pub base: sc_cli::PurgeChainCmd,

	/// Only delete the para chain database
	#[clap(long, aliases = &["para"])]
	pub parachain: bool,

	/// Only delete the relay chain database
	#[clap(long, aliases = &["relay"])]
	pub relaychain: bool,
}

impl PurgeChainCmd {
	/// Run the purge command
	pub fn run(
		&self,
		para_config: sc_service::Configuration,
		relay_config: sc_service::Configuration,
	) -> sc_cli::Result<()> {
		let databases = match (self.parachain, self.relaychain) {
			(true, true) | (false, false) => {
				vec![("parachain", para_config.database), ("relaychain", relay_config.database)]
			},
			(true, false) => vec![("parachain", para_config.database)],
			(false, true) => vec![("relaychain", relay_config.database)],
		};

		let db_paths = databases
			.iter()
			.map(|(chain_label, database)| {
				database.path().ok_or_else(|| {
					sc_cli::Error::Input(format!(
						"Cannot purge custom database implementation of: {}",
						chain_label,
					))
				})
			})
			.collect::<sc_cli::Result<Vec<_>>>()?;

		if !self.base.yes {
			for db_path in &db_paths {
				println!("{}", db_path.display());
			}
			print!("Are you sure to remove? [y/N]: ");
			io::stdout().flush().expect("failed to flush stdout");

			let mut input = String::new();
			io::stdin().read_line(&mut input)?;
			let input = input.trim();

			match input.chars().nth(0) {
				Some('y') | Some('Y') => {},
				_ => {
					println!("Aborted");
					return Ok(())
				},
			}
		}

		for db_path in &db_paths {
			match fs::remove_dir_all(&db_path) {
				Ok(_) => {
					println!("{:?} removed.", &db_path);
				},
				Err(ref err) if err.kind() == io::ErrorKind::NotFound => {
					eprintln!("{:?} did not exist.", &db_path);
				},
				Err(err) => return Err(err.into()),
			}
		}

		Ok(())
	}
}

impl sc_cli::CliConfiguration for PurgeChainCmd {
	fn shared_params(&self) -> &sc_cli::SharedParams {
		&self.base.shared_params
	}

	fn database_params(&self) -> Option<&sc_cli::DatabaseParams> {
		Some(&self.base.database_params)
	}
}

fn validate_relay_chain_url(arg: &str) -> Result<(), String> {
	let url = Url::parse(arg).map_err(|e| e.to_string())?;

	if url.scheme() == "ws" {
		Ok(())
	} else {
		Err(format!(
			"'{}' URL scheme not supported. Only websocket RPC is currently supported",
			url.scheme()
		))
	}
}

/// The `run` command used to run a node.
#[derive(Debug, Parser)]
pub struct RunCmd {
	/// The cumulus RunCmd inherents from sc_cli's
	#[clap(flatten)]
	pub base: sc_cli::RunCmd,

	/// Run node as collator.
	///
	/// Note that this is the same as running with `--validator`.
	#[clap(long, conflicts_with = "validator")]
	pub collator: bool,

	/// EXPERIMENTAL: Specify an URL to a relay chain full node to communicate with.
	#[clap(
		long,
		parse(try_from_str),
		validator = validate_relay_chain_url,
		conflicts_with_all = &["alice", "bob", "charlie", "dave", "eve", "ferdie", "one", "two"]	)
	]
	pub relay_chain_rpc_url: Option<Url>,
}

/// Options only relevant for collator nodes
#[derive(Clone, Debug)]
pub struct CollatorOptions {
	/// Location of relay chain full node
	pub relay_chain_rpc_url: Option<Url>,
}

/// A non-redundant version of the `RunCmd` that sets the `validator` field when the
/// original `RunCmd` had the `collator` field.
/// This is how we make `--collator` imply `--validator`.
pub struct NormalizedRunCmd {
	/// The cumulus RunCmd inherents from sc_cli's
	pub base: sc_cli::RunCmd,
}

impl RunCmd {
	/// Create a [`NormalizedRunCmd`] which merges the `collator` cli argument into `validator` to have only one.
	pub fn normalize(&self) -> NormalizedRunCmd {
		let mut new_base = self.base.clone();

		new_base.validator = self.base.validator || self.collator;

		NormalizedRunCmd { base: new_base }
	}

	/// Create [`CollatorOptions`] representing options only relevant to parachain collator nodes
	pub fn collator_options(&self) -> CollatorOptions {
		CollatorOptions { relay_chain_rpc_url: self.relay_chain_rpc_url.clone() }
	}
}

impl sc_cli::CliConfiguration for NormalizedRunCmd {
	fn shared_params(&self) -> &sc_cli::SharedParams {
		self.base.shared_params()
	}

	fn import_params(&self) -> Option<&sc_cli::ImportParams> {
		self.base.import_params()
	}

	fn network_params(&self) -> Option<&sc_cli::NetworkParams> {
		self.base.network_params()
	}

	fn keystore_params(&self) -> Option<&sc_cli::KeystoreParams> {
		self.base.keystore_params()
	}

	fn offchain_worker_params(&self) -> Option<&sc_cli::OffchainWorkerParams> {
		self.base.offchain_worker_params()
	}

	fn node_name(&self) -> sc_cli::Result<String> {
		self.base.node_name()
	}

	fn dev_key_seed(&self, is_dev: bool) -> sc_cli::Result<Option<String>> {
		self.base.dev_key_seed(is_dev)
	}

	fn telemetry_endpoints(
		&self,
		chain_spec: &Box<dyn sc_cli::ChainSpec>,
	) -> sc_cli::Result<Option<TelemetryEndpoints>> {
		self.base.telemetry_endpoints(chain_spec)
	}

	fn role(&self, is_dev: bool) -> sc_cli::Result<sc_cli::Role> {
		self.base.role(is_dev)
	}

	fn force_authoring(&self) -> sc_cli::Result<bool> {
		self.base.force_authoring()
	}

	fn prometheus_config(
		&self,
		default_listen_port: u16,
		chain_spec: &Box<dyn sc_cli::ChainSpec>,
	) -> sc_cli::Result<Option<PrometheusConfig>> {
		self.base.prometheus_config(default_listen_port, chain_spec)
	}

	fn disable_grandpa(&self) -> sc_cli::Result<bool> {
		self.base.disable_grandpa()
	}

	fn rpc_ws_max_connections(&self) -> sc_cli::Result<Option<usize>> {
		self.base.rpc_ws_max_connections()
	}

	fn rpc_cors(&self, is_dev: bool) -> sc_cli::Result<Option<Vec<String>>> {
		self.base.rpc_cors(is_dev)
	}

	fn rpc_http(&self, default_listen_port: u16) -> sc_cli::Result<Option<SocketAddr>> {
		self.base.rpc_http(default_listen_port)
	}

	fn rpc_ipc(&self) -> sc_cli::Result<Option<String>> {
		self.base.rpc_ipc()
	}

	fn rpc_ws(&self, default_listen_port: u16) -> sc_cli::Result<Option<SocketAddr>> {
		self.base.rpc_ws(default_listen_port)
	}

	fn rpc_methods(&self) -> sc_cli::Result<sc_service::config::RpcMethods> {
		self.base.rpc_methods()
	}

	fn rpc_max_payload(&self) -> sc_cli::Result<Option<usize>> {
		self.base.rpc_max_payload()
	}

	fn ws_max_out_buffer_capacity(&self) -> sc_cli::Result<Option<usize>> {
		self.base.ws_max_out_buffer_capacity()
	}

	fn transaction_pool(&self) -> sc_cli::Result<TransactionPoolOptions> {
		self.base.transaction_pool()
	}

	fn max_runtime_instances(&self) -> sc_cli::Result<Option<usize>> {
		self.base.max_runtime_instances()
	}

	fn runtime_cache_size(&self) -> sc_cli::Result<u8> {
		self.base.runtime_cache_size()
	}

	fn base_path(&self) -> sc_cli::Result<Option<BasePath>> {
		self.base.base_path()
	}
}
