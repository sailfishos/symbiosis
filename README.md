Sailfish OS TOH daemon
======================
Read more about TOH concepts [in Sailfish OS Wiki](https://docs.sailfishos.org/Develop/Hardware/#the-other-half-development).

This implements currently a minimal TOH daemon that exposes TOH information on
D-Bus for other services to consume.

***This is still work in progress.***

Use [Sailfish SDK](https://docs.sailfishos.org/Tools/Sailfish_SDK/) to build.

Future work
-----------
The important known missing parts are:

- Loading, unloading, binding and unbinding drivers for TOHs.
- More granular access control.
- Handling of larger memory chips.
- Proper interrupt handling for TOH interrupt pin.

Additionally there are many TODOs to implement all around the code base.
If you want to get your hands dirty, those are a good place to start.
