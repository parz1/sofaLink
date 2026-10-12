package dev.livingroom.streamlab;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;

/** Read-only setup query via ADB shell. No UI, frames, network or decoder allocation. */
public final class CapabilityProvider extends ContentProvider {
    public boolean onCreate(){return true;}
    public Bundle call(String method,String arg,Bundle extras) {
        if(!"media".equals(method))throw new IllegalArgumentException("Unknown capability query; update host and client together");
        return MediaCapabilities.query(getContext(),arg);
    }
    public Cursor query(Uri u,String[] p,String s,String[] a,String o){throw new UnsupportedOperationException();}
    public String getType(Uri u){return "application/json";}
    public Uri insert(Uri u,ContentValues v){throw new UnsupportedOperationException();}
    public int delete(Uri u,String s,String[] a){throw new UnsupportedOperationException();}
    public int update(Uri u,ContentValues v,String s,String[] a){throw new UnsupportedOperationException();}
}
