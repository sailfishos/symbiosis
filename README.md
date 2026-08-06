Sailfish OS TOH daemon
======================
Read more about TOH concepts [in Sailfish OS Wiki](https://docs.sailfishos.org/Develop/Hardware/#the-other-half-development).

This implements a TOH daemon that exposes TOH information on D-Bus for other
services to consume. It also allows to start and stop systemd units depending
on the connected TOH.

***This is still work in progress.***

Use [Sailfish SDK](https://docs.sailfishos.org/Tools/Sailfish_SDK/) to build.

For TOH implementors
--------------------
This service is TOH agnostic.
It does not implement TOH specific features.
Any TOHs should provide their own configuration that will affect the behaviour on TOH connect.
More complex features can be implemented with new services and kernel drivers.

Future work
-----------
The important known missing parts are:

- Loading, unloading, binding and unbinding drivers for TOHs.
- More granular access control.
- Handling of larger memory chips.
- Proper interrupt handling for TOH interrupt pin.

Additionally there are many TODOs to implement all around the code base.
If you want to get your hands dirty, those are a good place to start.
