// Copyright 2022 Parity Technologies (UK) Ltd.
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
mod cli;

use clap::Parser;
use cli::{Commands, RelayChainCli, TestCollatorCli};
use cumulus_client_service::genesis::generate_genesis_block;
use cumulus_primitives_core::{relay_chain::v2::CollatorPair, ParaId};
use cumulus_test_service::AnnounceBlockFn;
use polkadot_service::runtime_traits::AccountIdConversion;
use sc_cli::{CliConfiguration, SubstrateCli};
use sp_core::{hexdisplay::HexDisplay, Encode, Pair};
use sp_runtime::traits::Block;
use std::{io::Write, sync::Arc};

fn extract_genesis_wasm(chain_spec: &Box<dyn sc_service::ChainSpec>) -> Result<Vec<u8>, String> {
	let mut storage = chain_spec.build_storage()?;

	storage
		.top
		.remove(sp_core::storage::well_known_keys::CODE)
		.ok_or_else(|| "Could not find wasm file in genesis state!".into())
}

pub fn wrap_announce_block() -> Box<dyn FnOnce(AnnounceBlockFn) -> AnnounceBlockFn> {
	tracing::info!("Block announcements disabled.");
	Box::new(|_| {
		// Never announce any block
		Arc::new(|_, _| {})
	})
}

fn main() -> Result<(), sc_cli::Error> {
	let cli = TestCollatorCli::parse();

	match &cli.subcommand {
		Some(Commands::BuildSpec(cmd)) => {
			let runner = cli.create_runner(cmd)?;
			runner.sync_run(|config| cmd.run(config.chain_spec, config.network))
		},
		Some(Commands::ExportGenesisState(params)) => {
			let mut builder = sc_cli::LoggerBuilder::new("");
			builder.with_profiling(sc_tracing::TracingReceiver::Log, "");
			let _ = builder.init();

			let parachain_id = ParaId::from(params.parachain_id);
			let spec = Box::new(cumulus_test_service::get_chain_spec(parachain_id)) as Box<_>;
			let state_version = cumulus_test_service::runtime::VERSION.state_version();

			let block: parachains_common::Block = generate_genesis_block(&spec, state_version)?;
			let raw_header = block.header().encode();
			let output_buf = if params.raw {
				raw_header
			} else {
				format!("0x{:?}", HexDisplay::from(&block.header().encode())).into_bytes()
			};

			if let Some(output) = &params.output {
				std::fs::write(output, output_buf)?;
			} else {
				std::io::stdout().write_all(&output_buf)?;
			}

			Ok(())
		},
		Some(Commands::ExportGenesisWasm(params)) => {
			let mut builder = sc_cli::LoggerBuilder::new("");
			builder.with_profiling(sc_tracing::TracingReceiver::Log, "");
			let _ = builder.init();

			let parachain_id = ParaId::from(params.parachain_id);
			let spec = Box::new(cumulus_test_service::get_chain_spec(parachain_id)) as Box<_>;
			let raw_wasm_blob = extract_genesis_wasm(&spec)?;
			let output_buf = if params.raw {
				raw_wasm_blob
			} else {
				format!("0x{:?}", HexDisplay::from(&raw_wasm_blob)).into_bytes()
			};

			if let Some(output) = &params.output {
				std::fs::write(output, output_buf)?;
			} else {
				std::io::stdout().write_all(&output_buf)?;
			}

			Ok(())
		},
		None => {
			let mut builder = sc_cli::LoggerBuilder::new("");
			builder.with_colors(true);
			let _ = builder.init();

			let collator_options = cli.run.collator_options();
			let tokio_runtime = sc_cli::build_runtime()?;
			let tokio_handle = tokio_runtime.handle();
			let config = cli
				.run
				.normalize()
				.create_configuration(&cli, tokio_handle.clone())
				.expect("Should be able to generate config");

			let parachain_id = ParaId::from(cli.parachain_id);
			let polkadot_cli = RelayChainCli::new(
				&config,
				[RelayChainCli::executable_name().to_string()]
					.iter()
					.chain(cli.relaychain_args.iter()),
			);

			let parachain_account =
				AccountIdConversion::<polkadot_primitives::v2::AccountId>::into_account_truncating(
					&parachain_id,
				);

			let state_version =
				RelayChainCli::native_runtime_version(&config.chain_spec).state_version();

			let block: parachains_common::Block =
				generate_genesis_block(&config.chain_spec, state_version)
					.map_err(|e| format!("{:?}", e))?;
			let genesis_state = format!("0x{:?}", HexDisplay::from(&block.header().encode()));

			let tokio_handle = config.tokio_handle.clone();
			let polkadot_config =
				SubstrateCli::create_configuration(&polkadot_cli, &polkadot_cli, tokio_handle)
					.map_err(|err| format!("Relay chain argument error: {}", err))?;

			tracing::info!("Parachain id: {:?}", parachain_id);
			tracing::info!("Parachain Account: {}", parachain_account);
			tracing::info!("Parachain genesis state: {}", genesis_state);
			tracing::info!(
				"Is collating: {}",
				if config.role.is_authority() { "yes" } else { "no" }
			);

			let collator_key = Some(CollatorPair::generate().0);

			let consensus = cli
				.use_null_consensus
				.then(|| {
					tracing::info!("Using null consensus.");
					cumulus_test_service::Consensus::Null
				})
				.unwrap_or(cumulus_test_service::Consensus::RelayChain);

			let (mut task_manager, _, _, _, _) = tokio_runtime
				.block_on(cumulus_test_service::start_node_impl(
					config,
					collator_key,
					polkadot_config,
					parachain_id,
					cli.disable_block_announcements.then(wrap_announce_block),
					|_| Ok(jsonrpsee::RpcModule::new(())),
					consensus,
					collator_options,
				))
				.expect("could not create Cumulus test service");

			tokio_runtime
				.block_on(task_manager.future())
				.expect("Could not run service to completion");
			Ok(())
		},
	}
}
