import * as aws from "@pulumi/aws";
import * as cloudflare from "@pulumi/cloudflare";
import * as pulumi from "@pulumi/pulumi";
import { domain, stack, zireaelZoneId } from "./config";

// ---------------------------------------------------------------------------
// S3 Bucket
// ---------------------------------------------------------------------------
export const siteBucket = new aws.s3.BucketV2("sentinel-frontend-bucket", {});

new aws.s3.BucketPublicAccessBlock("sentinel-frontend-block", {
  bucket: siteBucket.id,
  blockPublicAcls: true,
  blockPublicPolicy: true,
  ignorePublicAcls: true,
  restrictPublicBuckets: true,
});

const oac = new aws.cloudfront.OriginAccessControl("sentinel-oac", {
  name: `sentinel-${stack}-oac`,
  originAccessControlOriginType: "s3",
  signingBehavior: "always",
  signingProtocol: "sigv4",
});

// ---------------------------------------------------------------------------
// ACM Certificate for Frontend
// ---------------------------------------------------------------------------
const frontendCert = new aws.acm.Certificate("sentinel-frontend-cert", {
  domainName: domain,
  validationMethod: "DNS",
});

const frontendCertVal = frontendCert.domainValidationOptions.apply(
  (opts) => opts[0],
);
const frontendCertValRecord = new cloudflare.Record(
  "sentinel-frontend-cert-validation",
  {
    zoneId: zireaelZoneId,
    name: frontendCertVal.apply((v) => v.resourceRecordName),
    type: frontendCertVal.apply((v) => v.resourceRecordType),
    content: frontendCertVal.apply((v) => v.resourceRecordValue),
    ttl: 60,
  },
);

const frontendCertValidated = new aws.acm.CertificateValidation(
  "sentinel-frontend-cert-validated",
  {
    certificateArn: frontendCert.arn,
    validationRecordFqdns: [frontendCertValRecord.name],
  },
);

// ---------------------------------------------------------------------------
// CloudFront Distribution
// ---------------------------------------------------------------------------
export const cdn = new aws.cloudfront.Distribution("sentinel-cdn", {
  enabled: true,
  defaultRootObject: "index.html",
  aliases: [domain],
  origins: [
    {
      domainName: siteBucket.bucketRegionalDomainName,
      originId: "s3",
      originAccessControlId: oac.id,
    },
  ],
  defaultCacheBehavior: {
    targetOriginId: "s3",
    viewerProtocolPolicy: "redirect-to-https",
    allowedMethods: ["GET", "HEAD", "OPTIONS"],
    cachedMethods: ["GET", "HEAD"],
    forwardedValues: { queryString: false, cookies: { forward: "none" } },
    compress: true,
  },
  customErrorResponses: [
    { errorCode: 404, responseCode: 200, responsePagePath: "/index.html" },
    { errorCode: 403, responseCode: 200, responsePagePath: "/index.html" },
  ],
  viewerCertificate: {
    acmCertificateArn: frontendCertValidated.certificateArn,
    sslSupportMethod: "sni-only",
    minimumProtocolVersion: "TLSv1.2_2021",
  },
  restrictions: { geoRestriction: { restrictionType: "none" } },
});

// S3 bucket policy: allow CloudFront OAC
new aws.s3.BucketPolicy("sentinel-frontend-policy", {
  bucket: siteBucket.id,
  policy: pulumi.all([siteBucket.arn, cdn.arn]).apply(([bucketArn, cdnArn]) =>
    JSON.stringify({
      Version: "2012-10-17",
      Statement: [
        {
          Effect: "Allow",
          Principal: { Service: "cloudfront.amazonaws.com" },
          Action: "s3:GetObject",
          Resource: `${bucketArn}/*`,
          Condition: { StringEquals: { "AWS:SourceArn": cdnArn } },
        },
      ],
    }),
  ),
});

// Cloudflare DNS for frontend
new cloudflare.Record("sentinel-frontend-dns", {
  zoneId: zireaelZoneId,
  name: domain,
  type: "CNAME",
  content: cdn.domainName,
  ttl: 1, // automatic
  proxied: false, // CloudFront handles TLS, Cloudflare proxy would double-terminate
});
