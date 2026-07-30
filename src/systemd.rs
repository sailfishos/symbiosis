// Copyright (c) 2026 Jolla Mobile Ltd

//! Running systemd units.

use crate::toh::config::parse::{Exec, SystemdUnit};
use futures::StreamExt;
use zbus::{connection::Builder, Connection, Error};
use zvariant::{OwnedObjectPath, OwnedValue, Str, Value};

mod proxies {
    //! Minimal proxies for systemd calls.
    use zbus::{fdo::Result, proxy};
    use zvariant::{OwnedObjectPath, OwnedValue};

    #[proxy(
        interface = "org.freedesktop.login1.Seat",
        default_service = "org.freedesktop.login1",
        default_path = "/org/freedesktop/login1/seat/seat0",
        gen_blocking = false
    )]
    pub(crate) trait Seat {
        #[zbus(property)]
        fn sessions(&self) -> Result<Vec<(String, OwnedObjectPath)>>;
    }

    #[proxy(
        interface = "org.freedesktop.login1.Session",
        default_service = "org.freedesktop.login1",
        gen_blocking = false
    )]
    pub(crate) trait Session {
        #[zbus(property)]
        fn user(&self) -> Result<(u32, OwnedObjectPath)>;
    }

    #[proxy(
        interface = "org.freedesktop.systemd1.Manager",
        default_service = "org.freedesktop.systemd1",
        default_path = "/org/freedesktop/systemd1",
        gen_blocking = false
    )]
    pub(crate) trait Manager {
        fn get_unit(&self, name: &str) -> Result<OwnedObjectPath>;
        fn start_transient_unit(
            &self,
            name: &str,
            mode: &str,
            properties: &Vec<(&str, OwnedValue)>,
            aux: &Vec<(String, Vec<(String, OwnedValue)>)>,
        ) -> Result<OwnedObjectPath>;
        fn start_unit(&self, name: &str, mode: &str) -> Result<OwnedObjectPath>;
        fn stop_unit(&self, name: &str, mode: &str) -> Result<OwnedObjectPath>;
        #[zbus(signal)]
        fn job_removed(
            &self,
            id: u32,
            job: OwnedObjectPath,
            unit: &str,
            result: &str,
        ) -> Result<()>;
    }

    #[proxy(
        interface = "org.freedesktop.systemd1.Unit",
        default_service = "org.freedesktop.systemd1",
        gen_blocking = false
    )]
    pub(crate) trait Unit {
        #[zbus(property)]
        fn can_stop(&self) -> Result<bool>;
        fn stop(&self, mode: &str) -> Result<OwnedObjectPath>;
    }
}

fn unit_name(unit: &SystemdUnit) -> String {
    use crate::toh::config::parse::Unit;
    // TODO: We have only .service units now
    assert!(matches!(
        unit.unit_type,
        Unit::Service | Unit::TransientService(..)
    ));
    let suffix = ".service";
    if unit.name.ends_with(&suffix) {
        unit.name.clone()
    } else {
        format!("{}{}", unit.name, suffix)
    }
}

fn exec_line(exec: &Exec) -> (&str, Vec<&str>, bool) {
    (exec.path(), exec.args(), false)
}

pub(crate) struct Manager<'p> {
    proxy: proxies::ManagerProxy<'p>,
}

impl<'p> Manager<'p> {
    pub async fn system() -> Result<Self, Error> {
        let connection = Connection::system().await?;
        Ok(Self {
            proxy: proxies::ManagerProxy::new(&connection).await?,
        })
    }

    pub async fn session() -> Result<Self, Error> {
        // Get user for session of seat0 to use in D-Bus object path
        let dbus = Connection::system().await?;
        let seat = proxies::SeatProxy::new(&dbus).await?;
        for (session, object_path) in seat.sessions().await? {
            if session == "1" {
                let session = proxies::SessionProxy::new(&dbus, object_path).await?;
                let (uid, _) = session.user().await?;

                // Build connection for the user
                let connection = Builder::address(
                    format!("unix:path=/run/user/{}/dbus/user_bus_socket", uid).as_str(),
                )?
                .build()
                .await?;

                return Ok(Self {
                    proxy: proxies::ManagerProxy::new(&connection).await?,
                });
            }
        }
        Err(zbus::Error::Failure(
            "No session 1 on seat0 found".to_owned(),
        ))
    }

    async fn wait_for_job(
        name: &str,
        mut jobs: proxies::JobRemovedStream<'_>,
        job: &OwnedObjectPath,
    ) -> Result<(), Error> {
        log::debug!("Waiting for {job} to finish");
        while let Some(removed) = jobs.next().await {
            let args = removed.args()?;
            log::debug!("Job {} removed", args.job());
            if args.job() == job {
                return match *args.result() {
                    "done" => {
                        log::debug!("Start of {name} finished successfully");
                        Ok(())
                    }
                    "canceled" | "failed" | "skipped" => {
                        Err(format!("Start of {name} {}", args.result()))
                    }
                    "timeout" => Err(format!("Timeout for start job of {name}")),
                    "dependency" => Err(format!("Start of {name} failed on dependency")),
                    other => Err(format!("Start of {name} failed for reason '{other}'")),
                }
                .map_err(Error::Failure);
            }
            // TODO: Abort after some time if job does not proceed
        }
        Err(Error::Failure(
            "Could not wait for job to finish".to_owned(),
        ))
    }

    pub async fn start_unit(&mut self, unit: &SystemdUnit) -> Result<(), Error> {
        use crate::toh::config::parse::Unit;
        let name = unit_name(unit);
        match &unit.unit_type {
            Unit::Service => {
                log::info!("Starting service {name}");
                let jobs = self.proxy.receive_job_removed().await?;
                let job = self.proxy.start_unit(&name, "replace").await?;
                Self::wait_for_job(&name, jobs, &job).await
            }
            Unit::TransientService(service) => {
                let mut properties = vec![
                    (
                        "Description",
                        OwnedValue::from(Str::from(service.description.as_str())),
                    ),
                    (
                        "Type",
                        OwnedValue::from(Str::from(service.service_type.to_string())),
                    ),
                    (
                        "ExecStart",
                        OwnedValue::try_from(Value::from([exec_line(&service.exec)].as_slice()))?,
                    ),
                ];
                if let Some(exec) = &service.exec_stop {
                    properties.push((
                        "ExecStop",
                        OwnedValue::try_from(Value::from([exec_line(exec)].as_slice()))?,
                    ))
                }
                log::info!("Starting transient service {name}");
                let jobs = self.proxy.receive_job_removed().await?;
                let job = self
                    .proxy
                    .start_transient_unit(&name, "replace", &properties, &Vec::new())
                    .await?;
                Self::wait_for_job(&name, jobs, &job).await
            }
        }
    }

    pub async fn stop_unit(&mut self, unit: &SystemdUnit) -> Result<(), Error> {
        let name = unit_name(unit);
        let object_path = match self.proxy.get_unit(&name).await {
            Ok(object_path) => object_path,
            Err(zbus::fdo::Error::ZBus(Error::MethodError(error_name, ..)))
                if error_name.as_str() == "org.freedesktop.systemd1.NoSuchUnit" =>
            {
                // The unit is gone, all ok
                return Ok(());
            }
            Err(error) => {
                return Err(error.into());
            }
        };
        let proxy = proxies::UnitProxy::new(self.proxy.inner().connection(), object_path).await?;
        if proxy.can_stop().await? {
            log::info!("Stopping unit {name}");
            let _job = proxy.stop("replace").await?;
        }
        Ok(())
    }
}
