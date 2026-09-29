org.sailfishos.tohd1
====================
Symbiosis provides `org.sailfishos.tohd1` bus name on _system bus_.

TOH info
--------
The D-Bus service provides _TOH info_ on path _/org/sailfishos/tohd1/toh_ via `org.sailfishos.tohd1.Toh` interface's properties.
The content there is a combination of known keys in _TOH payload data_ and _overrides_ from [configuration](configuration.md#override).

Note that TOH info is not always available.
It becomes available when TOH is attached, detected and possible configuration is read successfully.
To follow attaching and detaching of TOH, use the standard `org.freedesktop.DBus.ObjectManager.InterfacesAdded` and `org.freedesktop.DBus.ObjectManager.InterfacesRemoved` signals as defined in [D-Bus specification](https://dbus.freedesktop.org/doc/dbus-specification.html#standard-interfaces-objectmanager).

The properties are as in the following table:
| Property          | Description                                                                           |
|-------------------|---------------------------------------------------------------------------------------|
| `VendorId`        | Vendor ID from TOH header.                                                            |
| `ProductId`       | Product ID from TOH header.                                                           |
| `SerialNumber`    | Serial number from TOH payload.                                                       |
| `VendorName`      | Vendor name from TOH payload or config overrides.                                     |
| `ProductName`     | Product name from TOH payload or config overrides.                                    |
| `VendorWebsite`   | Vendor website from TOH payload or config overrides.                                  |
| `ProductWebsite`  | Product website from TOH payload or config overrides.                                 |
| `LeavePowerOn`    | Request to leave power on after reading payload from TOH payload or config overrides. |
| `PowerInputToh`   | Whether TOH provides power from TOH payload or config overrides.                      |
| `ExtraData`       | Any extra keys and values from config overrides.                                      |

Symbiosis places any unrecognized configuration keys from overrides to `ExtraData`.
This is a good place to add new TOH specific keys that do not conform to the two-letter specification of the payload data.

Borrowing i2c-dev character device
----------------------------------
In addition to TOH info, the interface provides access to [_i2c-dev character device_](https://www.kernel.org/doc/html/latest/i2c/dev-interface.html) that can be used for controlling TOH from services or apps.
Access is given to the binaries defined in [configuration](configuration.md#access), and always to _root user_.
The access works via borrowing principle, so that only one process can get it at a time.
Borrowing of the access turns on power output to TOH and power on the I²C/I3C bus.

Note that due to use of _I3C wrapper driver_, all target addresses must be listed [in configuration](configuration.md#devices) so that symbiosis can add devices for them and thus they can be used via i2c-dev character device.

When access is borrowed the process should be prepared to also drop the access when TOH is disconnected (e.g. the aforementioned `InterfacesRemoved` signal is received).
If the service is controlled via _systemd_ by symbiosis, then it will be stopped on TOH disconnect.
It can also return the borrow by calling `ReturnI2cDevAccess` any time, and depending on TOH configuration and previously given arguments on D-Bus this can turn off the power output to TOH.

This functionality is exposed via the following methods:
| Method                        | Description                                                                |
|-------------------------------|----------------------------------------------------------------------------|
| `BorrowI2cDevAccess`          | Borrows i2c-dev character device. Power is toggled as specified in config. |
| `BorrowI2cDevAccessWithPower` | Otherwise as above but power is set as specified by the argument.          |
| `ReturnI2cDevAccess`          | Return i2c-dev character device access.                                    |

Borrowing methods send back file descriptor that accesses TOH I²C bus directly.
Use `ioctl(fd, I2C_SLAVE, address)` to set the target device address.
Borrowing turns on 5V output on TOH and also powers on the bus enabling pull-ups.

`BorrowI2cDevAccessWithPower` is different from `BorrowI2cDevAccess` only so that it takes a boolean to tell if 5V output should be left on after returning access to the device.
This can useful if one wants to do one-off operation on the bus and leave the devices powered on.

When access to i2c-dev character device is lent, no other process can request it from the D-Bus interface.
Access is considered returned when the process that borrowed it disappears from system bus, or it returns the access by calling `ReturnI2cDevAccess`.
The process must stop using the file descriptor when it returns access.

Note that symbiosis cannot fully control the access i2c-dev character device.
It is possible to gain access to the device as root user outside symbiosis and symbiosis cannot control that.
It is also possible to share the file descriptor to other processes but that is not recommended.
Well-behaving clients should use the device only through symbiosis and follow the contract above.
Furthermore symbiosis allows access to the device without root user as long as the caller is allowed in configuration.
