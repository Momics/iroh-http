package com.iroh.http

import android.app.Activity
import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.util.Log
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONArray
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong

@InvokeArg
class BrowseStartArgs {
    lateinit var serviceName: String
}

@InvokeArg
class BrowsePollArgs {
    var browseId: Long = 0
}

@InvokeArg
class BrowseStopArgs {
    var browseId: Long = 0
}

@InvokeArg
class AdvertiseStartArgs {
    lateinit var serviceName: String
    lateinit var pk: String
    var relay: String? = null
}

@InvokeArg
class AdvertiseStopArgs {
    var advertiseId: Long = 0
}

@TauriPlugin
class IrohHttpPlugin(private val activity: Activity) : Plugin(activity) {

    private val nextBrowseId = AtomicLong(1)
    private val nextAdvertiseId = AtomicLong(1)

    private data class BrowseSession(
        val id: Long,
        val manager: NsdManager,
        val listener: NsdManager.DiscoveryListener,
        val pendingEvents: MutableList<JSObject> = mutableListOf(),
        val knownNodes: ConcurrentHashMap<String, String> = ConcurrentHashMap()
    )

    private data class AdvertiseSession(
        val id: Long,
        val manager: NsdManager,
        val listener: NsdManager.RegistrationListener
    )

    private val browseMap = ConcurrentHashMap<Long, BrowseSession>()
    private val advertiseMap = ConcurrentHashMap<Long, AdvertiseSession>()

    private fun nsd(): NsdManager? =
        activity.getSystemService(Context.NSD_SERVICE) as? NsdManager

    // ── Browse ────────────────────────────────────────────────────────────────

    @Command
    fun mdns_browse_start(invoke: Invoke) {
        val manager = nsd() ?: return invoke.reject("NsdManager unavailable")
        val args = invoke.parseArgs(BrowseStartArgs::class.java)
        val browseId = nextBrowseId.getAndIncrement()
        val serviceType = "_${args.serviceName}._udp"

        val listener = object : NsdManager.DiscoveryListener {
            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                Log.e("iroh-http-mdns", "browse $browseId start failed: $errorCode")
            }
            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {}
            override fun onDiscoveryStarted(serviceType: String) {}
            override fun onDiscoveryStopped(serviceType: String) {}

            override fun onServiceFound(serviceInfo: NsdServiceInfo) {
                val session = browseMap[browseId] ?: return
                manager.resolveService(serviceInfo, object : NsdManager.ResolveListener {
                    override fun onServiceResolved(resolved: NsdServiceInfo) {
                        val pk = resolved.attributes["pk"]?.let { String(it) }
                            ?: return
                        if (pk.isEmpty()) return

                        val key = resolved.serviceName
                        if (session.knownNodes[key] == pk) return

                        session.knownNodes[key] = pk
                        val addrs = JSONArray()
                        resolved.attributes["relay"]?.let { b ->
                            val relay = String(b)
                            if (relay.isNotEmpty()) addrs.put(relay)
                        }

                        val event = JSObject()
                        event.put("type", "discovered")
                        event.put("nodeId", pk)
                        event.put("addrs", addrs)
                        synchronized(session.pendingEvents) { session.pendingEvents.add(event) }
                    }

                    override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {}
                })
            }

            override fun onServiceLost(serviceInfo: NsdServiceInfo) {
                val session = browseMap[browseId] ?: return
                val pk = session.knownNodes.remove(serviceInfo.serviceName) ?: return
                val event = JSObject()
                event.put("type", "expired")
                event.put("nodeId", pk)
                event.put("addrs", JSONArray())
                synchronized(session.pendingEvents) { session.pendingEvents.add(event) }
            }
        }

        val session = BrowseSession(browseId, manager, listener)
        browseMap[browseId] = session

        try {
            manager.discoverServices(serviceType, NsdManager.PROTOCOL_DNS_SD, listener)
        } catch (e: Exception) {
            browseMap.remove(browseId)
            return invoke.reject("Discovery failed: ${e.message}")
        }

        val ret = JSObject()
        ret.put("browseId", browseId)
        invoke.resolve(ret)
    }

    @Command
    fun mdns_browse_poll(invoke: Invoke) {
        val args = invoke.parseArgs(BrowsePollArgs::class.java)
        val session = browseMap[args.browseId]
        val ret = JSObject()
        if (session == null) {
            ret.put("events", JSONArray())
        } else {
            val events: List<JSObject>
            synchronized(session.pendingEvents) {
                events = session.pendingEvents.toList()
                session.pendingEvents.clear()
            }
            val arr = JSONArray()
            events.forEach { arr.put(it) }
            ret.put("events", arr)
        }
        invoke.resolve(ret)
    }

    @Command
    fun mdns_browse_stop(invoke: Invoke) {
        val args = invoke.parseArgs(BrowseStopArgs::class.java)
        val session = browseMap.remove(args.browseId)
        if (session != null) {
            try { session.manager.stopServiceDiscovery(session.listener) } catch (_: Exception) {}
        }
        invoke.resolve()
    }

    // ── Advertise ─────────────────────────────────────────────────────────────

    @Command
    fun mdns_advertise_start(invoke: Invoke) {
        val manager = nsd() ?: return invoke.reject("NsdManager unavailable")
        val args = invoke.parseArgs(AdvertiseStartArgs::class.java)
        val advertiseId = nextAdvertiseId.getAndIncrement()
        val serviceType = "_${args.serviceName}._udp"

        val info = NsdServiceInfo().apply {
            serviceName = args.pk.take(63)
            this.serviceType = serviceType
            setPort(1)  // placeholder; iroh-http connections use node-ID, not port
            setAttribute("pk", args.pk)
            args.relay?.let { setAttribute("relay", it) }
        }

        val listener = object : NsdManager.RegistrationListener {
            override fun onServiceRegistered(serviceInfo: NsdServiceInfo) {}
            override fun onRegistrationFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
                Log.e("iroh-http-mdns", "advertise $advertiseId failed: $errorCode")
            }
            override fun onServiceUnregistered(serviceInfo: NsdServiceInfo) {}
            override fun onUnregistrationFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {}
        }

        advertiseMap[advertiseId] = AdvertiseSession(advertiseId, manager, listener)
        try {
            manager.registerService(info, NsdManager.PROTOCOL_DNS_SD, listener)
        } catch (e: Exception) {
            advertiseMap.remove(advertiseId)
            return invoke.reject("Registration failed: ${e.message}")
        }

        val ret = JSObject()
        ret.put("advertiseId", advertiseId)
        invoke.resolve(ret)
    }

    @Command
    fun mdns_advertise_stop(invoke: Invoke) {
        val args = invoke.parseArgs(AdvertiseStopArgs::class.java)
        val session = advertiseMap.remove(args.advertiseId)
        if (session != null) {
            try { session.manager.unregisterService(session.listener) } catch (_: Exception) {}
        }
        invoke.resolve()
    }
}
