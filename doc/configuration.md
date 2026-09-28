TOH configuration
=================
Symbiosis can be configured per TOH with [YAML](https://en.wikipedia.org/wiki/YAML) config files.

Config files are organized under _/usr/share/tohd-1/tohs/_ directory.
After TOH is successfully detected, symbiosis reads all files ending in _.yaml_ file extension from _/usr/share/tohd-1/tohs/VID/PID/_ directory, where _VID_ is _vendor ID_ and _PID_ is _product ID_ of the detected TOH.
The files are read in sorted file name order, which allows for example prefixing them with a number.
In some cases, later read files can overwrite configuration defined in earlier read files.

Vendor ID and product ID are formatted in lower case hexadecimal format and padded with zero (0) to four characters.
Thus TOH with vendor ID 1 and product id 49374 would have its configuration read from _/usr/share/tohd-1/tohs/0001/c0de/_.

Example configuration
---------------------
```yaml
override:
  # Just to provide the website url
  vendor-website: https://example.com
access:
  i2c-dev:
    - /usr/libexec/acme/controller
devices:
  # Exposes two addresses to i2c-dev
  - address: 0x45
  - address: 0x46
user-unit:
  description: Turn on the lights
  type: transient-service
  service-type: oneshot
  service-name: talk-to-the-controller
  service-exec:
    - /usr/libexec/acme/controller
    - --leave-powered
```

Override
--------
TOH configuration can specify so called overrides for TOH info via `override` config object.
This mainly changes how TOH is presented on D-Bus.
It allows to use values from configuration instead of TOH payload values from the memory chip.
Overrides can also have other values that are used as extra data for TOH info.
See [TOH info in D-Bus interface documentation](dbus_interface.md#toh-info) for more details on that.

Override values that override payload data:
| Override member    | Two-letter payload key |
|--------------------|------------------------|
| `vendor-name`      | `VN`                   |
| `product-name`     | `PN`                   |
| `vendor-website`   | `VS`                   |
| `product-website`  | `PS`                   |
| `leave-power-on`   | `PO`                   |
| `power-input-toh`  | `PI`                   |

Any other members are included as they are in extra data.

Access
------
Access control to TOH is provided via `access` object.

Currently this only supports one member, `i2c-dev`, that takes a list of executable paths to allow access for.
Processes created from those paths can borrow _i2c-dev character device_ as explained in [D-Bus interface documentation](dbus_interface.md#borrowing-i2c-dev-character-device).

Devices
-------
I²C devices can be configured via `devices` list.
It takes a list of objects with two members: `address` and `name`.
Defining `name` is optional.
`address` defines target device address to configure,
and `name` defines the name to use for the device.

Note that kernel may bind devices to drivers based on `name` and thus it shouldn't be set if TOH is controlled via i2c-dev character device.

System-unit and user-unit
-------------------------
_System and user units_ to start and stop on TOH attach and detach can be defined via `system-unit` and `user-unit` config objects.

Both system-unit and user-unit define some of the same members.
They have `type`, `service-name` and `run-on-start` members.
Additionally, they may have other members depending on the value of `type`.

`type` must be either set to `service` or to `transient-service`.
When `type` is `service`, the service set by `service-name` is started on TOH attach.
When `type` is `transient-service`, a transient service named by `service-name` is created and started instead.

`run-on-start` is a boolean that skips the service from starting when symbiosis starts if set to `false`.
This can be used for skipping the service from starting on boot, so it is only started right after TOH is attached to device.
By default, the services are also run when symbiosis starts.

Transient services must have also `description`, `service-type` and `service-exec` defined.
Optionally they can also define `service-exec-stop`.
`description` defines description for the unit.
`service-type` defines unit service type, which can be `simple` (default), `exec` or `oneshot`.
These are defined the same as in [_systemd_'s documentation](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html#Type=) and other types are currently unsupported.
`service-exec` is a list of strings that defines the program to run, matching _ExecStart_ in systemd.
The first string in the list must be an absolute path to an executable, and the rest are the arguments for the executable.
`service-exec-stop` is defined the same way but it is run on stop instead, matching _ExecStop_ in systemd.

Note that it is possible to define only one system unit and one user unit in a single configuration file.
