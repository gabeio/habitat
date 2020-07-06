use super::util::{CacheKeyPath,
                  ConfigOptCacheKeyPath,
                  ConfigOptPkgIdent,
                  ConfigOptRemoteSup,
                  PkgIdent,
                  RemoteSup};
use crate::error::{Error,
                   Result};
use configopt::ConfigOpt;
use habitat_core::{os::process::ShutdownTimeout,
                   package::PackageIdent,
                   service::{BindingMode,
                             HealthCheckInterval,
                             ServiceBind,
                             ServiceGroup},
                   ChannelIdent};
use habitat_sup_protocol::{ctl,
                           types::UpdateCondition};
use std::{convert::TryFrom,
          iter::FromIterator};
use structopt::StructOpt;
use url::Url;

/// Commands relating to Habitat services
#[derive(ConfigOpt, StructOpt)]
#[structopt(no_version)]
#[allow(clippy::large_enum_variant)]
pub enum Svc {
    Key(Key),
    /// Load a service to be started and supervised by Habitat from a package identifier. If an
    /// installed package doesn't satisfy the given package identifier, a suitable package will be
    /// installed from Builder.
    #[structopt(no_version)]
    Load(Load),
    #[structopt(no_version)]
    Update(Update),
    /// Start a loaded, but stopped, Habitat service.
    Start {
        #[structopt(flatten)]
        pkg_ident:  PkgIdent,
        #[structopt(flatten)]
        remote_sup: RemoteSup,
    },
    /// Query the status of Habitat services
    Status {
        /// A package identifier (ex: core/redis, core/busybox-static/1.42.2)
        #[structopt(name = "PKG_IDENT")]
        pkg_ident:  Option<PackageIdent>,
        #[structopt(flatten)]
        remote_sup: RemoteSup,
    },
    /// Stop a running Habitat service.
    Stop {
        #[structopt(flatten)]
        pkg_ident:        PkgIdent,
        #[structopt(flatten)]
        remote_sup:       RemoteSup,
        /// The delay in seconds after sending the shutdown signal to wait before killing the
        /// service process
        ///
        /// The default value is set in the packages plan file.
        #[structopt(name = "SHUTDOWN_TIMEOUT", long = "shutdown-timeout")]
        shutdown_timeout: Option<ShutdownTimeout>,
    },
    /// Unload a service loaded by the Habitat Supervisor. If the service is running it will
    /// additionally be stopped.
    Unload {
        #[structopt(flatten)]
        pkg_ident:        PkgIdent,
        #[structopt(flatten)]
        remote_sup:       RemoteSup,
        /// The delay in seconds after sending the shutdown signal to wait before killing the
        /// service process
        ///
        /// The default value is set in the packages plan file.
        #[structopt(name = "SHUTDOWN_TIMEOUT", long = "shutdown-timeout")]
        shutdown_timeout: Option<ShutdownTimeout>,
    },
}

#[derive(ConfigOpt, StructOpt)]
#[structopt(no_version)]
/// Commands relating to Habitat service keys
pub enum Key {
    /// Generates a Habitat service key
    Generate {
        /// Target service group service.group[@organization] (ex: redis.default or
        /// foo.default@bazcorp)
        #[structopt(name = "SERVICE_GROUP")]
        service_group:  ServiceGroup,
        /// The service organization
        #[structopt(name = "ORG")]
        org:            Option<String>,
        #[structopt(flatten)]
        cache_key_path: CacheKeyPath,
    },
}

lazy_static::lazy_static! {
    static ref CHANNEL_IDENT_DEFAULT: String = String::from(ChannelIdent::default().as_str());
}

#[derive(ConfigOpt, StructOpt, Deserialize)]
#[configopt(attrs(serde))]
#[serde(deny_unknown_fields)]
#[structopt(no_version, rename_all = "screamingsnake")]
#[allow(dead_code)]
pub struct SharedLoad {
    /// Receive updates from the specified release channel
    #[structopt(long = "channel", default_value = &*CHANNEL_IDENT_DEFAULT)]
    pub channel:               ChannelIdent,
    /// Specify an alternate Builder endpoint. If not specified, the value will be taken from
    /// the HAB_BLDR_URL environment variable if defined. (default: https://bldr.habitat.sh)
    // TODO (DM): This should probably use `env` and `default_value`
    // TODO (DM): Nested flattens do no work
    #[structopt(name = "BLDR_URL", short = "u", long = "url")]
    pub bldr_url:              Option<Url>,
    /// The service group with shared config and topology
    #[structopt(long = "group", default_value = "default")]
    pub group:                 String,
    /// Service topology
    #[structopt(long = "topology",
            short = "t",
            possible_values = &["standalone", "leader"])]
    pub topology:              Option<habitat_sup_protocol::types::Topology>,
    /// The update strategy
    #[structopt(long = "strategy",
                short = "s",
                default_value = "none",
                possible_values = &["none", "at-once", "rolling"])]
    pub strategy:              habitat_sup_protocol::types::UpdateStrategy,
    /// The condition dictating when this service should update
    ///
    /// latest: Runs the latest package that can be found in the configured channel and local
    /// packages.
    ///
    /// track-channel: Always run what is at the head of a given channel. This enables service
    /// rollback where demoting a package from a channel will cause the package to rollback to
    /// an older version of the package. A ramification of enabling this condition is packages
    /// newer than the package at the head of the channel will be automatically uninstalled
    /// during a service rollback.
    #[structopt(long = "update-condition",
                default_value = UpdateCondition::Latest.as_str(),
                possible_values = UpdateCondition::VARIANTS)]
    pub update_condition:      UpdateCondition,
    /// One or more service groups to bind to a configuration
    #[structopt(long = "bind")]
    #[serde(default)]
    pub bind:                  Vec<ServiceBind>,
    /// Governs how the presence or absence of binds affects service startup
    ///
    /// strict: blocks startup until all binds are present.
    #[structopt(long = "binding-mode",
                default_value = "strict",
                possible_values = &["strict", "relaxed"])]
    pub binding_mode:          habitat_sup_protocol::types::BindingMode,
    /// The interval in seconds on which to run health checks
    // We would prefer to use `HealthCheckInterval`. However, `HealthCheckInterval` uses a map based
    // serialization format. We want to allow the user to simply specify a `u64` to be consistent
    // with the CLI, but we cannot change the serialization because the spec file depends on the map
    // based format.
    #[structopt(long = "health-check-interval", short = "i", default_value = "30")]
    pub health_check_interval: u64,
    /// The delay in seconds after sending the shutdown signal to wait before killing the service
    /// process
    ///
    /// The default value can be set in the packages plan file.
    #[structopt(long = "shutdown-timeout")]
    pub shutdown_timeout:      Option<ShutdownTimeout>,
    #[cfg(target_os = "windows")]
    /// Password of the service user
    #[structopt(long = "password")]
    pub password:              Option<String>,
    // TODO (DM): This flag can eventually be removed.
    // See https://github.com/habitat-sh/habitat/issues/7339
    /// DEPRECATED
    #[structopt(long = "application", short = "a", takes_value = false, hidden = true)]
    #[serde(skip)]
    pub application:           Vec<String>,
    // TODO (DM): This flag can eventually be removed.
    // See https://github.com/habitat-sh/habitat/issues/7339
    /// DEPRECATED
    #[structopt(long = "environment", short = "e", takes_value = false, hidden = true)]
    #[serde(skip)]
    pub environment:           Vec<String>,
}

#[derive(ConfigOpt, StructOpt, Deserialize)]
#[configopt(attrs(serde))]
#[serde(deny_unknown_fields)]
#[structopt(no_version)]
#[allow(dead_code)]
pub struct Load {
    #[structopt(flatten)]
    #[serde(flatten)]
    pkg_ident:   PkgIdent,
    /// Load or reload an already loaded service. If the service was previously loaded and
    /// running this operation will also restart the service
    #[structopt(name = "FORCE", short = "f", long = "force")]
    force:       bool,
    #[structopt(flatten)]
    #[serde(flatten)]
    remote_sup:  RemoteSup,
    #[structopt(flatten)]
    #[serde(flatten)]
    shared_load: SharedLoad,
}

/// Update how the Supervisor manages an already-running
/// service. Depending on the given changes, they may be able to
/// be applied without restarting the service.
#[derive(ConfigOpt, StructOpt, Deserialize)]
#[configopt(attrs(serde))]
#[serde(deny_unknown_fields)]
#[structopt(name = "update", no_version, rename_all = "screamingsnake")]
#[allow(dead_code)]
pub struct Update {
    #[structopt(flatten)]
    #[serde(flatten)]
    pkg_ident: PkgIdent,

    #[structopt(flatten)]
    #[serde(flatten)]
    pub remote_sup: RemoteSup,

    // This is some unfortunate duplication... everything below this
    // should basically be identical to SharedLoad, except that we
    // don't want to have default values, and everything should be
    // optional.
    /// Receive updates from the specified release channel
    #[structopt(long = "channel")]
    pub channel: Option<ChannelIdent>,

    /// Specify an alternate Builder endpoint.
    #[structopt(name = "BLDR_URL", short = "u", long = "url")]
    pub bldr_url: Option<Url>,

    /// The service group with shared config and topology
    #[structopt(long = "group")]
    pub group: Option<String>,

    /// Service topology
    #[structopt(long = "topology",
                short = "t",
                possible_values = &["standalone", "leader"])]
    pub topology: Option<habitat_sup_protocol::types::Topology>,

    /// The update strategy
    #[structopt(long = "strategy",
                short = "s",
                possible_values = &["none", "at-once", "rolling"])]
    pub strategy: Option<habitat_sup_protocol::types::UpdateStrategy>,

    /// The condition dictating when this service should update
    ///
    /// latest: Runs the latest package that can be found in the configured channel and local
    /// packages.
    ///
    /// track-channel: Always run what is at the head of a given channel. This enables service
    /// rollback where demoting a package from a channel will cause the package to rollback to
    /// an older version of the package. A ramification of enabling this condition is packages
    /// newer than the package at the head of the channel will be automatically uninstalled
    /// during a service rollback.
    #[structopt(long = "update-condition",
                possible_values = UpdateCondition::VARIANTS)]
    pub update_condition: Option<UpdateCondition>,

    /// One or more service groups to bind to a configuration
    #[structopt(long = "bind")]
    #[serde(default)]
    pub bind: Option<Vec<ServiceBind>>,

    /// Governs how the presence or absence of binds affects service startup
    ///
    /// strict: blocks startup until all binds are present.
    #[structopt(long = "binding-mode",
                possible_values = &["strict", "relaxed"])]
    pub binding_mode: Option<BindingMode>,

    /// The interval in seconds on which to run health checks
    // We can use `HealthCheckInterval` here (cf. `SharedLoad` above),
    // because we don't have to worry about serialization here.
    #[structopt(long = "health-check-interval", short = "i")]
    pub health_check_interval: Option<HealthCheckInterval>,

    /// The delay in seconds after sending the shutdown signal to wait before killing the service
    /// process
    ///
    /// The default value can be set in the packages plan file.
    #[structopt(long = "shutdown-timeout")]
    pub shutdown_timeout: Option<ShutdownTimeout>,

    /// Password of the service user
    #[cfg(target_os = "windows")]
    #[structopt(long = "password")]
    pub password: Option<String>,
}

impl TryFrom<Update> for ctl::SvcUpdate {
    type Error = Error;

    fn try_from(u: Update) -> Result<Self> {
        let mut msg = ctl::SvcUpdate::default();

        msg.ident = Some(From::from(u.pkg_ident.pkg_ident));
        // We are explicitly *not* using the environment variable as a
        // fallback.
        msg.bldr_url = u.bldr_url.map(|u| u.to_string());
        msg.bldr_channel = u.channel.map(Into::into);
        msg.binds = u.bind.map(FromIterator::from_iter);
        msg.group = u.group;
        msg.health_check_interval = u.health_check_interval.map(From::from);
        msg.binding_mode = u.binding_mode.map(|v| v as i32);
        msg.topology = u.topology.map(|v| v as i32);
        msg.update_strategy = u.strategy.map(|v| v as i32);
        msg.update_condition = u.update_condition.map(|v| v as i32);
        msg.shutdown_timeout = u.shutdown_timeout.map(u32::from);

        #[cfg(target_os = "windows")]
        {
            msg.svc_encrypted_password = u.password;
        }

        // Compiler-assisted validation that we've checked everything
        if let ctl::SvcUpdate { ident: _,
                                binds: None,
                                binding_mode: None,
                                bldr_url: None,
                                bldr_channel: None,
                                group: None,
                                svc_encrypted_password: None,
                                topology: None,
                                update_strategy: None,
                                health_check_interval: None,
                                shutdown_timeout: None,
                                update_condition: None, } = &msg
        {
            Err(Error::ArgumentError("No fields specified for update".to_string()))
        } else {
            Ok(msg)
        }
    }
}
